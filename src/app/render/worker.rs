//! One persistent worker; render requests take priority over coalesced query preparation.
use super::*;
use super::{
    cache::Cache,
    engine::{Outcome, perform},
};
use std::sync::{MutexGuard, mpsc};

// Match the enqueue lock order: result destination before diagnostics. Keeping
// this guard until publication prevents an enqueue from inverting those locks.
enum Publication<'a> {
    Render(MutexGuard<'a, Option<(u64, Result<Rendered, String>)>>),
    Prepare(MutexGuard<'a, Option<(u64, Result<PreparedProject, String>)>>),
    Playback(MutexGuard<'a, Option<(u64, Result<Arc<Playback>, String>)>>),
}
struct PlaybackWork {
    request: Request,
    session: Arc<Playback>,
    profile: RenderProfile,
    started: Instant,
    submitted: Instant,
}
fn merge_operators(
    profile: &mut RenderProfile,
    values: Vec<roicepeak::graph::profiling::OperatorTiming>,
) {
    for value in values {
        let value = super::profiling::OperatorStats::from(value);
        if let Some(previous) = profile.operators.iter_mut().find(|previous| {
            previous.kind == value.kind && previous.input_channels == value.input_channels
        }) {
            previous.maximum_output_channels = previous
                .maximum_output_channels
                .max(value.maximum_output_channels);
            previous.calls = previous.calls.saturating_add(value.calls);
            previous.failures = previous.failures.saturating_add(value.failures);
            previous.input_elements = previous.input_elements.saturating_add(value.input_elements);
            previous.output_elements = previous
                .output_elements
                .saturating_add(value.output_elements);
            previous.inclusive_seconds += value.inclusive_seconds;
            previous.exclusive_seconds += value.exclusive_seconds;
            previous.inclusive_multiply_adds = previous
                .inclusive_multiply_adds
                .saturating_add(value.inclusive_multiply_adds);
            previous.exclusive_multiply_adds = previous
                .exclusive_multiply_adds
                .saturating_add(value.exclusive_multiply_adds);
        } else if profile.operators.len() < 64 {
            profile.operators.push(value);
        } else {
            profile.operators_truncated = true;
        }
    }
    profile.operators.sort_by(|a, b| {
        b.exclusive_seconds
            .total_cmp(&a.exclusive_seconds)
            .then(a.kind.cmp(&b.kind))
            .then(a.input_channels.cmp(&b.input_channels))
    });
}
impl Default for Worker {
    fn default() -> Self {
        let shared = Arc::new(Shared::default());
        let worker_shared = shared.clone();
        let (wake, receiver) = mpsc::sync_channel(1);
        let playback_wake = wake.clone();
        let start = std::thread::Builder::new()
            .name("roicepeak-synthesis".into())
            .spawn(move || {
                let mut cache = Cache::default();
                let mut active: Option<PlaybackWork> = None;
                while receiver.recv().is_ok() {
                    if worker_shared.closed.load(Ordering::Acquire) {
                        break;
                    }
                    loop {
                        let job = worker_shared.pending.lock().ok().and_then(|mut p| p.take());
                        let Some(job) = job else {
                            if let Some(work) = active.as_mut() {
                                if checkpoint(
                                    &worker_shared,
                                    work.session.generation,
                                    JobKind::Playback,
                                )
                                .is_err()
                                {
                                    work.session.cancel();
                                    work.profile.finished(
                                        "cancelled_or_superseded",
                                        work.started.elapsed(),
                                        work.submitted.elapsed(),
                                    );
                                    if let Ok(mut ledger) = worker_shared.diagnostics.lock() {
                                        ledger.record(&work.profile);
                                    }
                                    active = None;
                                    continue;
                                }
                                if let Some(index) = work.session.next() {
                                    let result = std::panic::catch_unwind(
                                        std::panic::AssertUnwindSafe(|| {
                                            let (result, operators) =
                                                roicepeak::graph::profiling::capture(|| {
                                                    super::playback_unit::render(
                                                        &work.request,
                                                        &work.session,
                                                        index,
                                                        &mut cache,
                                                        &worker_shared,
                                                        &mut work.profile,
                                                    )
                                                });
                                            merge_operators(&mut work.profile, operators);
                                            result
                                        }),
                                    )
                                    .unwrap_or_else(|_| {
                                        cache = Cache::default();
                                        work.profile.panicked = true;
                                        Err(Error("Playback synthesis worker failed".into()))
                                    });
                                    if checkpoint(
                                        &worker_shared,
                                        work.session.generation,
                                        JobKind::Playback,
                                    )
                                    .is_err()
                                    {
                                        work.session.cancel();
                                    } else {
                                        match result {
                                            Ok(pcm) => {
                                                if let Err(error) =
                                                    work.session.complete(index, pcm)
                                                {
                                                    work.session.fail(error);
                                                }
                                            }
                                            Err(error) => work.session.fail(error.to_string()),
                                        }
                                    }
                                    let finished = work.session.finished();
                                    let status = if work.session.cancelled() {
                                        "cancelled_or_superseded"
                                    } else if work.session.failure().is_some() {
                                        "failed"
                                    } else if finished {
                                        "succeeded"
                                    } else {
                                        "rendering_sentences"
                                    };
                                    work.profile.finished(
                                        status,
                                        work.started.elapsed(),
                                        work.submitted.elapsed(),
                                    );
                                    if finished {
                                        if work.session.failure().is_none()
                                            && !work.session.cancelled()
                                        {
                                            work.profile.output_samples = work.session.plan.samples;
                                        }
                                        work.profile.sample_rate = SAMPLE_RATE;
                                        let _ = worker_shared.playback_busy.compare_exchange(
                                            work.session.generation,
                                            0,
                                            Ordering::AcqRel,
                                            Ordering::Acquire,
                                        );
                                    }
                                    if let Ok(mut ledger) = worker_shared.diagnostics.lock() {
                                        ledger.record(&work.profile);
                                    }
                                    if finished {
                                        active = None;
                                    }
                                    continue;
                                }
                            }
                            break;
                        };
                        let generation = job.generation;
                        let kind = job.kind;
                        let playback_request = (kind == JobKind::Playback).then(|| Request {
                            config: job.request.config.clone(),
                            project: job.request.project.clone(),
                            user_dictionary: job.request.user_dictionary.clone(),
                        });
                        let started = Instant::now();
                        let mut profile = job.profile;
                        let mut progress = job.progress;
                        profile.started(job.submitted.elapsed());
                        if let Ok(mut ledger) = worker_shared.diagnostics.lock() {
                            ledger.record(&profile);
                        }
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let (result, operators) = roicepeak::graph::profiling::capture(|| {
                                perform(
                                    job.request,
                                    &mut cache,
                                    &worker_shared,
                                    generation,
                                    kind,
                                    &mut profile,
                                    progress.as_mut(),
                                )
                            });
                            profile.operators = operators.into_iter().map(Into::into).collect();
                            result
                        }))
                        .unwrap_or_else(|_| {
                            profile.panicked = true;
                            cache = Cache::default();
                            Err(Error(
                                "Synthesis worker failed; resources will reload".into(),
                            ))
                        })
                        .map_err(|e| e.to_string());
                        let result = if kind == JobKind::Playback {
                            result.and_then(|outcome| match outcome {
                                Outcome::Prepare(plan) => {
                                    if job.playback_block.is_some_and(|id| {
                                        !plan
                                            .blocks
                                            .iter()
                                            .any(|block| block.id == id && block.duration > 0.0)
                                    }) {
                                        Err("The selected block has no prepared audio".into())
                                    } else {
                                        Playback::new(plan, generation, playback_wake.clone())
                                            .map(Outcome::Playback)
                                    }
                                }
                                _ => Err("Unexpected playback plan result".into()),
                            })
                        } else {
                            result
                        };
                        // Acquire potentially contended locks before final timing.
                        // Enqueue also takes result locks before diagnostics.
                        let mut publication = match kind {
                            JobKind::Render => {
                                worker_shared.result.lock().ok().map(Publication::Render)
                            }
                            JobKind::Prepare => worker_shared
                                .prepare_result
                                .lock()
                                .ok()
                                .map(Publication::Prepare),
                            JobKind::Playback => worker_shared
                                .playback_result
                                .lock()
                                .ok()
                                .map(Publication::Playback),
                        };
                        let mut ledger = worker_shared.diagnostics.lock().ok();
                        let current =
                            kind.current(&worker_shared).load(Ordering::Acquire) == generation;
                        let status = if !current {
                            "cancelled_or_superseded"
                        } else if result.is_ok() && kind == JobKind::Playback {
                            "playback_planned"
                        } else if result.is_ok() {
                            "succeeded"
                        } else {
                            "failed"
                        };
                        profile.finished(status, started.elapsed(), job.submitted.elapsed());
                        let result = result.map(|mut outcome| {
                            match &mut outcome {
                                Outcome::Render(rendered) => {
                                    profile.output_samples = rendered.pcm.len();
                                    profile.sample_rate = rendered.rate;
                                    rendered.profile = profile.clone();
                                }
                                Outcome::Prepare(prepared) => {
                                    prepared.profile = profile.clone();
                                }
                                Outcome::Playback(_) => {}
                            }
                            outcome
                        });
                        // Install final in-memory state before consumers can see
                        // the result. No file I/O occurs while holding this guard.
                        if let Some(ledger) = ledger.as_mut() {
                            ledger.record_memory(&profile);
                        }
                        if kind.current(&worker_shared).load(Ordering::Acquire) == generation {
                            match publication.as_mut() {
                                Some(Publication::Render(destination)) => {
                                    **destination = Some((
                                        generation,
                                        result.and_then(|outcome| match outcome {
                                            Outcome::Render(rendered) => Ok(rendered),
                                            _ => {
                                                Err("Unexpected preparation result for render"
                                                    .into())
                                            }
                                        }),
                                    ));
                                }
                                Some(Publication::Prepare(destination)) => {
                                    **destination = Some((
                                        generation,
                                        result.and_then(|outcome| match outcome {
                                            Outcome::Prepare(prepared) => Ok(prepared),
                                            _ => {
                                                Err("Unexpected render result for preparation"
                                                    .into())
                                            }
                                        }),
                                    ));
                                }
                                Some(Publication::Playback(destination)) => {
                                    **destination = Some((
                                        generation,
                                        result.and_then(|outcome| match outcome {
                                            Outcome::Playback(session) => {
                                                if let Some(previous) = active.take() {
                                                    previous.session.cancel();
                                                }
                                                if let Ok(mut live) =
                                                    worker_shared.playback_session.lock()
                                                {
                                                    *live = Some(Arc::downgrade(&session));
                                                }
                                                let start = job
                                                    .playback_block
                                                    .and_then(|id| {
                                                        session
                                                            .plan
                                                            .blocks
                                                            .iter()
                                                            .find(|block| block.id == id)
                                                    })
                                                    .map_or(job.playback_start, |block| {
                                                        block.start
                                                    });
                                                session.prefetch(
                                                    (start.max(0.0) * f64::from(SAMPLE_RATE))
                                                        as usize,
                                                );
                                                active = Some(PlaybackWork {
                                                    request: playback_request.unwrap(),
                                                    session: Arc::clone(&session),
                                                    profile: profile.clone(),
                                                    started,
                                                    submitted: job.submitted,
                                                });
                                                Ok(session)
                                            }
                                            _ => Err("Unexpected playback source result".into()),
                                        }),
                                    ));
                                }
                                None => {}
                            }
                        }
                        drop(ledger);
                        drop(publication);
                        if kind != JobKind::Playback
                            || active
                                .as_ref()
                                .is_none_or(|work| work.session.generation != generation)
                        {
                            let _ = kind.busy(&worker_shared).compare_exchange(
                                generation,
                                0,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            );
                        }
                        // Audio/query results and idle state are visible before
                        // diagnostic serialization or disk I/O can block here.
                        if let Ok(mut ledger) = worker_shared.diagnostics.lock() {
                            ledger.persist();
                        }
                    }
                }
            });
        Self {
            shared,
            wake,
            start_error: start.err().map(|e| e.to_string()),
        }
    }
}
impl Worker {
    pub fn busy(&self) -> bool {
        self.shared.busy.load(Ordering::Acquire) != 0
    }
    pub fn preparing(&self) -> bool {
        self.shared.preparing.load(Ordering::Acquire) != 0
    }
    pub fn submit(&mut self, request: Request) -> Result<(), String> {
        self.enqueue(request, JobKind::Render, 0.0, None, None)
            .map(|_| ())
    }
    /// Coalesce the latest full project snapshot. This job performs no acoustic/vocoder work.
    pub fn prepare(&mut self, request: Request) -> Result<(), String> {
        self.enqueue(request, JobKind::Prepare, 0.0, None, None)
            .map(|_| ())
    }
    pub fn start_playback(&mut self, request: Request, start: f64) -> Result<(), String> {
        if !start.is_finite() || start < 0.0 {
            return Err("Invalid playback start".into());
        }
        self.enqueue(request, JobKind::Playback, start, None, None)
            .map(|_| ())
    }
    pub fn start_block_playback(&mut self, request: Request, block: u64) -> Result<(), String> {
        self.enqueue(request, JobKind::Playback, 0.0, Some(block), None)
            .map(|_| ())
    }
    pub fn submit_with_progress(&mut self, plan: ProgressPlan) -> Result<RenderProgress, String> {
        let (request, pending) = plan.into_job();
        let prefix = Arc::clone(&pending.prefix);
        let valid = Arc::clone(&pending.valid);
        let total = pending.total;
        let generation = self.enqueue(request, JobKind::Render, 0.0, None, Some(pending))?;
        Ok(RenderProgress {
            generation,
            shared: Arc::downgrade(&self.shared),
            prefix,
            valid,
            total,
        })
    }
    fn enqueue(
        &mut self,
        request: Request,
        kind: JobKind,
        playback_start: f64,
        playback_block: Option<u64>,
        progress: Option<super::progress::Pending>,
    ) -> Result<u64, String> {
        if let Some(error) = &self.start_error {
            return Err(error.clone());
        }
        request.config.validate()?;
        request.project.validate()?;
        request.user_dictionary.validate()?;
        request
            .project
            .validate_controls(request.config.value_policy)?;
        let submitted = Instant::now();
        let mut profile = RenderProfile::new(&request);
        profile.job_kind = kind.name().into();
        let mut pending = self
            .shared
            .pending
            .lock()
            .map_err(|_| "Synthesis queue unavailable")?;
        // Clear results under their own locks before publishing a new generation.
        let mut result = self
            .shared
            .result
            .lock()
            .map_err(|_| "Synthesis result unavailable")?;
        let mut prepare_result = self
            .shared
            .prepare_result
            .lock()
            .map_err(|_| "Preparation result unavailable")?;
        let mut playback_result = self
            .shared
            .playback_result
            .lock()
            .map_err(|_| "Playback result unavailable")?;
        let previous = self
            .shared
            .sequence
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .map_err(|_| "Synthesis generation exhausted")?;
        let generation = previous + 1;
        profile.generation = generation;
        kind.current(&self.shared)
            .store(generation, Ordering::Release);
        kind.busy(&self.shared).store(generation, Ordering::Release);
        let job = Job {
            generation,
            kind,
            request,
            submitted,
            profile: profile.clone(),
            playback_start,
            playback_block,
            progress: progress
                .map(|pending| pending.bind(generation, Arc::downgrade(&self.shared))),
        };
        let mut replaced = Vec::new();
        match kind {
            JobKind::Render => {
                *result = None;
                *prepare_result = None;
                self.shared.prepare_generation.store(0, Ordering::Release);
                self.shared.preparing.store(0, Ordering::Release);
                replaced.extend(pending.prepare.take());
                replaced.extend(pending.render.replace(job));
            }
            JobKind::Prepare => {
                *prepare_result = None;
                replaced.extend(pending.prepare.replace(job));
            }
            JobKind::Playback => {
                *playback_result = None;
                *prepare_result = None;
                self.shared.prepare_generation.store(0, Ordering::Release);
                self.shared.preparing.store(0, Ordering::Release);
                replaced.extend(pending.prepare.take());
                replaced.extend(pending.playback.replace(job));
                if let Ok(live) = self.shared.playback_session.lock() {
                    if let Some(session) = live.as_ref().and_then(std::sync::Weak::upgrade) {
                        session.cancel();
                    }
                }
            }
        }
        if let Ok(mut ledger) = self.shared.diagnostics.lock() {
            for mut old in replaced {
                old.profile.finished(
                    "superseded_before_start",
                    Default::default(),
                    old.submitted.elapsed(),
                );
                ledger.record(&old.profile);
            }
            ledger.record(&profile);
        }
        drop(prepare_result);
        drop(playback_result);
        drop(result);
        drop(pending);
        match self.wake.try_send(()) {
            Ok(()) | Err(mpsc::TrySendError::Full(())) => Ok(generation),
            Err(mpsc::TrySendError::Disconnected(())) => {
                kind.busy(&self.shared).store(0, Ordering::Release);
                profile.finished(
                    "worker_unavailable",
                    Default::default(),
                    submitted.elapsed(),
                );
                if let Ok(mut ledger) = self.shared.diagnostics.lock() {
                    ledger.record(&profile);
                }
                Err("Synthesis worker is unavailable".into())
            }
        }
    }
    pub fn poll(&mut self) -> Option<Result<Rendered, String>> {
        let result = self.shared.result.lock().ok()?.take()?;
        (result.0 == self.shared.generation.load(Ordering::Acquire)).then_some(result.1)
    }
    pub fn poll_prepare(&mut self) -> Option<Result<PreparedProject, String>> {
        let result = self.shared.prepare_result.lock().ok()?.take()?;
        (result.0 == self.shared.prepare_generation.load(Ordering::Acquire)).then_some(result.1)
    }
    pub fn poll_playback(&mut self) -> Option<Result<Arc<Playback>, String>> {
        let result = self.shared.playback_result.lock().ok()?.take()?;
        (result.0 == self.shared.playback_generation.load(Ordering::Acquire)).then_some(result.1)
    }
    pub fn cancel(&mut self) {
        self.shared.generation.store(0, Ordering::Release);
        self.shared.prepare_generation.store(0, Ordering::Release);
        self.shared.playback_generation.store(0, Ordering::Release);
        self.shared.playback_busy.store(0, Ordering::Release);
        if let Ok(live) = self.shared.playback_session.lock() {
            if let Some(session) = live.as_ref().and_then(std::sync::Weak::upgrade) {
                session.cancel();
            }
        }
        self.shared.busy.store(0, Ordering::Release);
        self.shared.preparing.store(0, Ordering::Release);
        if let Ok(mut pending) = self.shared.pending.lock() {
            while let Some(mut job) = pending.take() {
                job.profile.finished(
                    "cancelled_before_start",
                    Default::default(),
                    job.submitted.elapsed(),
                );
                if let Ok(mut ledger) = self.shared.diagnostics.lock() {
                    ledger.record(&job.profile);
                }
            }
        }
        if let Ok(mut result) = self.shared.result.lock() {
            *result = None;
        }
        if let Ok(mut result) = self.shared.prepare_result.lock() {
            *result = None;
        }
        if let Ok(mut result) = self.shared.playback_result.lock() {
            *result = None;
        }
        let _ = self.wake.try_send(());
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        self.shared.closed.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
    }
}
