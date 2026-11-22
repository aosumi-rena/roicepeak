use super::{
    JobKind, Playback, RenderProfile, Request, SAMPLE_RATE, Shared, cache::Cache, checkpoint,
    playback_unit, unit, worker::merge_operators,
};
use roicepeak::{Error, Result, graph::profiling};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, atomic::Ordering, mpsc},
    thread::{self, JoinHandle},
    time::Instant,
};

const MIN_OVERLAP_WORKERS: usize = 4;
const MAX_OVERLAP_WORKERS: usize = 8;

struct Completed {
    audio: Result<unit::RenderedAudio>,
    profile: RenderProfile,
    operators: Vec<profiling::OperatorTiming>,
}
struct Task {
    index: usize,
    result: mpsc::Receiver<Completed>,
    thread: Option<JoinHandle<()>>,
}
impl Drop for Task {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(super) struct Work {
    request: Arc<Request>,
    pub(super) session: Arc<Playback>,
    profile: RenderProfile,
    started: Instant,
    submitted: Instant,
    worker_limit: usize,
    prefetch: bool,
    tasks: [Option<Task>; MAX_OVERLAP_WORKERS],
}
impl Work {
    pub(super) fn new(
        request: Arc<Request>,
        session: Arc<Playback>,
        profile: RenderProfile,
        started: Instant,
        submitted: Instant,
    ) -> Self {
        let available = thread::available_parallelism().map_or(1, |count| count.get());
        Self {
            request,
            session,
            profile,
            started,
            submitted,
            worker_limit: available.clamp(MIN_OVERLAP_WORKERS, MAX_OVERLAP_WORKERS),
            prefetch: available >= MIN_OVERLAP_WORKERS,
            tasks: std::array::from_fn(|_| None),
        }
    }

    pub(super) fn step(
        &mut self,
        cache: &mut Cache,
        shared: &Arc<Shared>,
        wake: &mpsc::SyncSender<()>,
    ) -> bool {
        if !self.current(shared) {
            self.session.cancel();
            self.report(shared);
            return true;
        }
        for task in &mut self.tasks {
            let Some(running) = task.as_ref() else {
                continue;
            };
            let result = match running.result.try_recv() {
                Ok(completed) => Ok(completed),
                Err(mpsc::TryRecvError::Empty) => continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err(Error("Playback renderer stopped".into()))
                }
            };
            let index = running.index;
            *task = None;
            self.finish_audio(index, result, cache, shared);
            return true;
        }
        let Some(slot) = self.tasks[..self.worker_limit]
            .iter()
            .position(Option::is_none)
        else {
            return false;
        };
        let active = self
            .tasks
            .each_ref()
            .map(|task| task.as_ref().map(|task| task.index));
        let prefetch = self.prefetch && self.session.initial_read();
        let Some(index) = self.session.next(&active, prefetch) else {
            return false;
        };
        let prepared = catch_unwind(AssertUnwindSafe(|| {
            let (result, operators) = profiling::capture(|| {
                playback_unit::prepare(
                    &self.request,
                    &self.session,
                    index,
                    cache,
                    shared,
                    &mut self.profile,
                )
            });
            merge_operators(&mut self.profile, operators);
            result
        }))
        .unwrap_or_else(|_| {
            *cache = Cache::default();
            self.profile.panicked = true;
            Err(Error("Playback synthesis worker failed".into()))
        });
        match prepared {
            Ok(unit::Prepared::Ready(output)) => {
                self.finish(index, playback_unit::pcm(output), shared);
            }
            Ok(unit::Prepared::Audio(audio)) if prefetch || self.session.has_overlap(index) => {
                let session = Arc::clone(&self.session);
                let worker_shared = Arc::clone(shared);
                let wake = wake.clone();
                let (send, result) = mpsc::channel();
                let thread = thread::Builder::new()
                    .name("roicepeak-playback".into())
                    .spawn(move || {
                        let completed = run(*audio, &session, index, &worker_shared);
                        let _ = send.send(completed);
                        let _ = wake.try_send(());
                    });
                match thread {
                    Ok(thread) => {
                        self.tasks[slot] = Some(Task {
                            index,
                            result,
                            thread: Some(thread),
                        })
                    }
                    Err(error) => {
                        self.finish(index, Err(Error(error.to_string())), shared.as_ref())
                    }
                }
            }
            Ok(unit::Prepared::Audio(audio)) => {
                let completed = run(*audio, &self.session, index, shared);
                self.finish_audio(index, Ok(completed), cache, shared);
            }
            Err(error) => self.finish(index, Err(error), shared),
        }
        true
    }

    fn current(&self, shared: &Shared) -> bool {
        !self.session.cancelled()
            && checkpoint(shared, self.session.generation, JobKind::Playback).is_ok()
    }

    fn finish_audio(
        &mut self,
        index: usize,
        completed: Result<Completed>,
        cache: &mut Cache,
        shared: &Shared,
    ) {
        let result = completed.and_then(|completed| {
            self.profile
                .baseline_calls
                .merge(completed.profile.baseline_calls);
            self.profile
                .edited_calls
                .merge(completed.profile.edited_calls);
            self.profile.panicked |= completed.profile.panicked;
            merge_operators(&mut self.profile, completed.operators);
            let rendered = completed.audio?;
            if self.current(shared) && self.session.failure().is_none() {
                if let Some(base) = rendered.baseline {
                    cache.remember(base, self.session.generation);
                }
            }
            playback_unit::pcm(rendered.output)
        });
        self.finish(index, result, shared);
    }

    fn finish(
        &mut self,
        index: usize,
        result: Result<super::streaming::PcmRange>,
        shared: &Shared,
    ) {
        if !self.current(shared) {
            self.session.cancel();
        } else if let Err(error) = result
            .map_err(|error| error.to_string())
            .and_then(|pcm| self.session.complete(index, pcm))
        {
            self.session.fail(error);
        }
        self.report(shared);
    }

    fn report(&mut self, shared: &Shared) {
        let status = if self.session.cancelled() {
            "cancelled_or_superseded"
        } else if self.session.failure().is_some() {
            "failed"
        } else if self.session.finished() {
            self.profile.output_samples = self.session.plan.samples;
            "succeeded"
        } else {
            "rendering_sentences"
        };
        self.profile
            .finished(status, self.started.elapsed(), self.submitted.elapsed());
        if self.session.finished() {
            self.profile.sample_rate = SAMPLE_RATE;
            let _ = shared.playback_busy.compare_exchange(
                self.session.generation,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        if let Ok(mut ledger) = shared.diagnostics.lock() {
            ledger.record(&self.profile);
        }
    }
}

fn run(audio: unit::Audio, session: &Playback, index: usize, shared: &Shared) -> Completed {
    let mut profile = RenderProfile::default();
    let (result, operators) = catch_unwind(AssertUnwindSafe(|| {
        profiling::capture(|| playback_unit::render(audio, session, index, shared, &mut profile))
    }))
    .unwrap_or_else(|_| {
        profile.panicked = true;
        (
            Err(Error("Playback synthesis worker failed".into())),
            Vec::new(),
        )
    });
    Completed {
        audio: result,
        profile,
        operators,
    }
}
