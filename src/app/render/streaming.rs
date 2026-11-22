use super::{PreparedProject, SAMPLE_RATE};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
        mpsc::SyncSender,
    },
    time::Duration,
};

struct Unit {
    state: AtomicU8,
    seek_preroll: AtomicUsize,
    pcm: OnceLock<PcmRange>,
    prefix: Mutex<Prefix>,
}

pub(super) struct PcmRange {
    pub(super) start: usize,
    pub(super) samples: Arc<Vec<f32>>,
}

#[derive(Default)]
struct Prefix {
    start: Option<usize>,
    samples: Vec<f32>,
}

pub struct Playback {
    pub generation: u64,
    pub plan: Arc<PreparedProject>,
    initial_sample: usize,
    initial_read: AtomicBool,
    units: Vec<Unit>,
    queue: Mutex<VecDeque<usize>>,
    signal: (Mutex<u64>, Condvar),
    wake: SyncSender<()>,
    cancelled: AtomicBool,
    failure: OnceLock<String>,
    error_taken: AtomicBool,
}

impl Playback {
    pub(super) fn new(
        plan: Arc<PreparedProject>,
        generation: u64,
        wake: SyncSender<()>,
        start: f64,
    ) -> Result<Arc<Self>, String> {
        if plan.samples == 0 || plan.samples > super::MAX_AUDIO {
            return Err("Project contains no bounded playback audio".into());
        }
        let retained = plan
            .units
            .iter()
            .try_fold(0usize, |total, unit| {
                total.checked_add((unit.duration * f64::from(SAMPLE_RATE)).round() as usize)
            })
            .ok_or("Playback memory size overflow")?;
        if retained > super::MAX_AUDIO {
            return Err("Combined sentence audio exceeds the playback memory budget".into());
        }
        let initial_sample = crate::app::audio::seek_sample(start, SAMPLE_RATE, plan.samples, true);
        let initial_sample = if initial_sample < plan.samples {
            initial_sample
        } else {
            0
        };
        let units = plan
            .units
            .iter()
            .map(|_| Unit {
                state: AtomicU8::new(0),
                seek_preroll: AtomicUsize::new(0),
                pcm: OnceLock::new(),
                prefix: Mutex::new(Prefix::default()),
            })
            .collect();
        Ok(Arc::new(Self {
            generation,
            plan,
            initial_sample,
            initial_read: AtomicBool::new(false),
            units,
            queue: Mutex::new(VecDeque::new()),
            signal: (Mutex::new(0), Condvar::new()),
            wake,
            cancelled: AtomicBool::new(false),
            failure: OnceLock::new(),
            error_taken: AtomicBool::new(false),
        }))
    }
    pub fn duration(&self) -> f64 {
        self.plan.samples as f64 / f64::from(SAMPLE_RATE)
    }
    pub(super) fn seek_target(&self, index: usize) -> usize {
        let unit = &self.plan.units[index];
        let length = (unit.duration * f64::from(SAMPLE_RATE)).round() as usize;
        let local = self.initial_sample.saturating_sub(unit.sample_start);
        if local < length { local } else { 0 }
    }

    pub(crate) fn covers_seek(&self, seconds: f64) -> bool {
        let frame = crate::app::audio::seek_sample(seconds, SAMPLE_RATE, self.plan.samples, true);
        if frame.saturating_add(1) < self.initial_sample {
            return self.units.iter().enumerate().all(|(index, unit)| {
                self.seek_target(index) == 0
                    || unit.pcm.get().map_or_else(
                        || {
                            unit.prefix
                                .lock()
                                .is_ok_and(|prefix| prefix.start == Some(0))
                        },
                        |pcm| pcm.start == 0,
                    )
            });
        }
        self.units.iter().enumerate().all(|(index, unit)| {
            let planned = &self.plan.units[index];
            let length = (planned.duration * f64::from(SAMPLE_RATE)).round() as usize;
            let preroll = unit.seek_preroll.load(Ordering::Acquire);
            let Some(local) = frame.checked_sub(planned.sample_start) else {
                return true;
            };
            if preroll == 0 || local >= length || unit.pcm.get().is_some() {
                return true;
            }
            unit.prefix.lock().is_ok_and(|prefix| {
                let end = prefix.start.unwrap_or(0) + prefix.samples.len();
                unit.pcm.get().is_some()
                    || local.saturating_sub(preroll) <= end.max(self.seek_target(index))
            })
        })
    }
    pub(super) fn set_seek_preroll(&self, index: usize, samples: Option<usize>) {
        self.units[index]
            .seek_preroll
            .store(samples.unwrap_or(0), Ordering::Release);
    }
    pub fn ready_units(&self) -> usize {
        self.units
            .iter()
            .filter(|unit| unit.pcm.get().is_some())
            .count()
    }
    pub fn total_units(&self) -> usize {
        self.units.len()
    }
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::AcqRel) {
            self.notify();
            let _ = self.wake.try_send(());
        }
    }
    pub fn failure(&self) -> Option<&str> {
        self.failure.get().map(String::as_str)
    }
    pub fn take_error(&self) -> Option<String> {
        let error = self.failure.get()?;
        (!self.error_taken.swap(true, Ordering::AcqRel)).then(|| error.clone())
    }
    pub(super) fn fail(&self, error: String) {
        let _ = self.failure.set(error);
        self.notify();
    }
    pub fn notify(&self) {
        if let Ok(mut sequence) = self.signal.0.lock() {
            *sequence = sequence.wrapping_add(1);
        }
        self.signal.1.notify_all();
    }
    pub fn wait(&self) {
        if let Ok(sequence) = self.signal.0.lock() {
            let _ = self
                .signal
                .1
                .wait_timeout(sequence, Duration::from_millis(10));
        }
    }
    pub fn prefetch(&self, cursor: usize) {
        self.demand(cursor, cursor.saturating_add(SAMPLE_RATE as usize));
    }
    fn sample_range(&self, index: usize) -> std::ops::Range<usize> {
        let unit = &self.plan.units[index];
        let samples = (unit.duration * f64::from(SAMPLE_RATE)).round() as usize;
        unit.sample_start..unit.sample_start.saturating_add(samples)
    }
    fn overlaps(&self, index: usize, other: usize) -> bool {
        let first = self.sample_range(index);
        let second = self.sample_range(other);
        first.start.max(second.start).max(self.initial_sample) < first.end.min(second.end)
    }
    pub(super) fn has_overlap(&self, index: usize) -> bool {
        self.units.iter().enumerate().any(|(other, unit)| {
            other != index && unit.state.load(Ordering::Acquire) != 3 && self.overlaps(index, other)
        })
    }
    pub(super) fn prime(&self) {
        let cursor = self.initial_sample;
        self.prefetch(cursor);
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        let range = |index| self.sample_range(index);
        let Some(first) = queue.iter().map(|&index| range(index).start).min() else {
            return;
        };
        let anchor = first.max(cursor);
        let last_overlap = queue
            .iter()
            .map(|&index| range(index))
            .filter(|clip| clip.start <= anchor && clip.end > anchor)
            .map(|clip| clip.end)
            .max();
        if let Some(last_overlap) = last_overlap {
            queue.make_contiguous().sort_by_key(|&index| {
                let clip = range(index);
                let overlaps = clip.start < last_overlap;
                let remaining = if overlaps {
                    clip.end.saturating_sub(clip.start.max(anchor))
                } else {
                    0
                };
                (!overlaps, remaining, index)
            });
        }
    }
    pub fn demand(&self, start: usize, end: usize) {
        if self.cancelled() || self.failure().is_some() {
            return;
        }
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        let mut scheduled = false;
        for (index, unit) in self.units.iter().enumerate() {
            let range = self.sample_range(index);
            if range.start <= end
                && range.end > start
                && unit
                    .state
                    .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
            {
                queue.push_back(index);
                scheduled = true;
            }
        }
        drop(queue);
        if scheduled {
            let _ = self.wake.try_send(());
        }
    }
    pub(super) fn initial_read(&self) -> bool {
        self.initial_read.load(Ordering::Acquire)
    }

    pub(super) fn next(&self, active: &[Option<usize>], prefetch: bool) -> Option<usize> {
        if self.cancelled() || self.failure().is_some() {
            return None;
        }
        let mut queue = self.queue.lock().ok()?;
        let mut running = active.iter().flatten().copied();
        let index = if let Some(first) = running.next() {
            if let Some(queued) = queue.iter().position(|&index| {
                active
                    .iter()
                    .flatten()
                    .any(|&running| self.overlaps(running, index))
            }) {
                queue.remove(queued)?
            } else if prefetch && running.next().is_none() {
                queue.pop_front().or_else(|| {
                    (1..self.units.len())
                        .map(|offset| (first + offset) % self.units.len())
                        .find(|&index| self.units[index].state.load(Ordering::Acquire) == 0)
                })?
            } else {
                return None;
            }
        } else {
            queue.pop_front()?
        };
        self.units[index].state.store(2, Ordering::Release);
        Some(index)
    }
    pub(super) fn append(&self, index: usize, start: usize, samples: &[f32]) -> Result<(), String> {
        let planned = self
            .plan
            .units
            .get(index)
            .ok_or("Sentence PCM disagrees with its prepared playback plan")?;
        let unit = &self.units[index];
        let expected = (planned.duration * f64::from(SAMPLE_RATE)).round() as usize;
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err("Sentence PCM disagrees with its prepared playback plan".into());
        }
        if self.cancelled() {
            return Ok(());
        }
        let mut prefix = unit
            .prefix
            .lock()
            .map_err(|_| "Sentence PCM prefix unavailable")?;
        if unit.pcm.get().is_some() {
            return Err("Sentence PCM was published twice".into());
        }
        let initial = prefix.start.unwrap_or(start);
        if initial.checked_add(prefix.samples.len()) != Some(start)
            || start
                .checked_add(samples.len())
                .is_none_or(|end| end > expected)
        {
            return Err("Sentence PCM disagrees with its prepared playback plan".into());
        }
        if self.cancelled() {
            return Ok(());
        }
        prefix.start = Some(initial);
        prefix.samples.extend_from_slice(samples);
        drop(prefix);
        if !samples.is_empty() {
            self.notify();
        }
        Ok(())
    }
    pub(super) fn complete(&self, index: usize, pcm: PcmRange) -> Result<(), String> {
        let planned = self
            .plan
            .units
            .get(index)
            .ok_or("Sentence PCM disagrees with its prepared playback plan")?;
        let unit = &self.units[index];
        let expected = (planned.duration * f64::from(SAMPLE_RATE)).round() as usize;
        if pcm.start.checked_add(pcm.samples.len()) != Some(expected)
            || pcm.samples.iter().any(|sample| !sample.is_finite())
        {
            return Err("Sentence PCM disagrees with its prepared playback plan".into());
        }
        if self.cancelled() {
            return Ok(());
        }
        let mut prefix = unit
            .prefix
            .lock()
            .map_err(|_| "Sentence PCM prefix unavailable")?;
        if prefix.start.is_some_and(|start| start != pcm.start)
            || prefix.samples.len() > pcm.samples.len()
            || prefix
                .samples
                .iter()
                .zip(pcm.samples.iter())
                .any(|(a, b)| a.to_bits() != b.to_bits())
        {
            return Err("Sentence PCM changed its published playback prefix".into());
        }
        if self.cancelled() {
            return Ok(());
        }
        unit.pcm
            .set(pcm)
            .map_err(|_| "Sentence PCM was published twice")?;
        *prefix = Prefix::default();
        drop(prefix);
        unit.state.store(3, Ordering::Release);
        let mut queue = self
            .queue
            .lock()
            .map_err(|_| "Playback queue unavailable")?;
        for offset in 1..=self.units.len() {
            let next = (index + offset) % self.units.len();
            if self.units[next]
                .state
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                queue.push_back(next);
                break;
            }
        }
        drop(queue);
        self.notify();
        Ok(())
    }
    pub(super) fn finished(&self) -> bool {
        self.failure().is_some()
            || self.cancelled()
            || self
                .units
                .iter()
                .all(|unit| unit.state.load(Ordering::Acquire) == 3)
    }
    pub fn mix_window(&self, start: usize, end: usize) -> Result<Option<Vec<f32>>, String> {
        if self.cancelled() {
            return Err("Playback cancelled".into());
        }
        if let Some(error) = self.failure() {
            return Err(error.into());
        }
        if start > end || end > self.plan.samples {
            return Err("Playback source window is outside the project".into());
        }
        self.demand(start, end.saturating_add(SAMPLE_RATE as usize));
        let mut output = vec![0.0; end - start];
        for (index, unit) in self.plan.units.iter().enumerate() {
            let length = (unit.duration * f64::from(SAMPLE_RATE)).round() as usize;
            let first = start.max(unit.sample_start);
            let last = end.min(unit.sample_start.saturating_add(length));
            if first >= last {
                continue;
            }
            let mut mix = |pcm_start: usize, pcm: &[f32]| -> Result<bool, String> {
                let Some(from) = (first - unit.sample_start).checked_sub(pcm_start) else {
                    return Ok(false);
                };
                let Some(input) = pcm.get(from..from + (last - first)) else {
                    return Ok(false);
                };
                for (destination, source) in
                    output[first - start..last - start].iter_mut().zip(input)
                {
                    *destination += *source;
                    if !destination.is_finite() {
                        return Err("Non-finite mixed playback sample".into());
                    }
                }
                Ok(true)
            };
            let storage = &self.units[index];
            let ready = if let Some(pcm) = storage.pcm.get() {
                mix(pcm.start, &pcm.samples)?
            } else {
                let prefix = storage
                    .prefix
                    .lock()
                    .map_err(|_| "Sentence PCM prefix unavailable")?;
                if let Some(pcm) = storage.pcm.get() {
                    drop(prefix);
                    mix(pcm.start, &pcm.samples)?
                } else if let Some(start) = prefix.start {
                    mix(start, &prefix.samples)?
                } else {
                    false
                }
            };
            if !ready {
                return Ok(None);
            }
        }
        if start <= self.initial_sample
            && self.initial_sample < end
            && self
                .initial_read
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        {
            let _ = self.wake.try_send(());
        }
        Ok(Some(output))
    }
}
