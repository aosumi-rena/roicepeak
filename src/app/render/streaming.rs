use super::{PreparedProject, SAMPLE_RATE};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::SyncSender,
    },
    time::Duration,
};

struct Unit {
    state: AtomicU8,
    pcm: OnceLock<Arc<Vec<f32>>>,
}

pub struct Playback {
    pub generation: u64,
    pub plan: PreparedProject,
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
        plan: PreparedProject,
        generation: u64,
        wake: SyncSender<()>,
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
        let units = plan
            .units
            .iter()
            .map(|_| Unit {
                state: AtomicU8::new(0),
                pcm: OnceLock::new(),
            })
            .collect();
        Ok(Arc::new(Self {
            generation,
            plan,
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
        self.cancelled.store(true, Ordering::Release);
        self.notify();
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
    pub fn demand(&self, start: usize, end: usize) {
        if self.cancelled() || self.failure().is_some() {
            return;
        }
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        let mut scheduled = false;
        for (index, unit) in self.plan.units.iter().enumerate() {
            let length = (unit.duration * f64::from(SAMPLE_RATE)).round() as usize;
            if unit.sample_start <= end
                && unit.sample_start.saturating_add(length) > start
                && self.units[index]
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
    pub(super) fn next(&self) -> Option<usize> {
        if self.cancelled() || self.failure().is_some() {
            return None;
        }
        let index = self.queue.lock().ok()?.pop_front()?;
        self.units[index].state.store(2, Ordering::Release);
        Some(index)
    }
    pub(super) fn complete(&self, index: usize, pcm: Arc<Vec<f32>>) -> Result<(), String> {
        let expected = (self.plan.units[index].duration * f64::from(SAMPLE_RATE)).round() as usize;
        if pcm.len() != expected || pcm.iter().any(|sample| !sample.is_finite()) {
            return Err("Sentence PCM disagrees with its prepared playback plan".into());
        }
        if self.cancelled() {
            return Ok(());
        }
        self.units[index]
            .pcm
            .set(pcm)
            .map_err(|_| "Sentence PCM was published twice")?;
        self.units[index].state.store(3, Ordering::Release);
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
        self.demand(start, end);
        for (index, unit) in self.plan.units.iter().enumerate() {
            let length = (unit.duration * f64::from(SAMPLE_RATE)).round() as usize;
            if unit.sample_start < end
                && unit.sample_start.saturating_add(length) > start
                && self.units[index].pcm.get().is_none()
            {
                return Ok(None);
            }
        }
        let mut output = vec![0.0; end - start];
        for (index, unit) in self.plan.units.iter().enumerate() {
            let Some(pcm) = self.units[index].pcm.get() else {
                continue;
            };
            let first = start.max(unit.sample_start);
            let last = end.min(unit.sample_start.saturating_add(pcm.len()));
            if first >= last {
                continue;
            }
            for (destination, source) in output[first - start..last - start]
                .iter_mut()
                .zip(&pcm[first - unit.sample_start..last - unit.sample_start])
            {
                *destination += *source;
                if !destination.is_finite() {
                    return Err("Non-finite mixed playback sample".into());
                }
            }
        }
        Ok(Some(output))
    }
}
