use super::*;
use crate::app::render::{Playback, SAMPLE_RATE};
use std::{sync::atomic::AtomicU8, thread::JoinHandle};

const CAPACITY: usize = 8192;
const QUANTUM: usize = 256;
struct Slot {
    value: AtomicU32,
    epoch: AtomicU32,
    position: AtomicU32,
}
struct Ring {
    slots: Box<[Slot]>,
    read: AtomicUsize,
    write: AtomicUsize,
}
impl Ring {
    fn new() -> Self {
        Self {
            slots: (0..CAPACITY)
                .map(|_| Slot {
                    value: AtomicU32::new(0),
                    epoch: AtomicU32::new(0),
                    position: AtomicU32::new(0),
                })
                .collect(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }
    fn free(&self) -> usize {
        CAPACITY.saturating_sub(
            self.write
                .load(Ordering::Relaxed)
                .wrapping_sub(self.read.load(Ordering::Acquire)),
        )
    }
    fn push(&self, epoch: u32, position: u32, value: f32) -> bool {
        let write = self.write.load(Ordering::Relaxed);
        if write.wrapping_sub(self.read.load(Ordering::Acquire)) >= CAPACITY {
            return false;
        }
        let slot = &self.slots[write % CAPACITY];
        slot.value.store(value.to_bits(), Ordering::Relaxed);
        slot.epoch.store(epoch, Ordering::Relaxed);
        slot.position.store(position, Ordering::Relaxed);
        self.write.store(write.wrapping_add(1), Ordering::Release);
        true
    }
    fn pop(&self, epoch: u32, position: u32) -> Option<f32> {
        for _ in 0..CAPACITY {
            let read = self.read.load(Ordering::Relaxed);
            if read == self.write.load(Ordering::Acquire) {
                return None;
            }
            let slot = &self.slots[read % CAPACITY];
            let tag = slot.epoch.load(Ordering::Relaxed);
            let at = slot.position.load(Ordering::Relaxed);
            if tag == epoch && at > position {
                return None;
            }
            let value = f32::from_bits(slot.value.load(Ordering::Relaxed));
            self.read.store(read.wrapping_add(1), Ordering::Release);
            if tag == epoch && at == position {
                return Some(value);
            }
        }
        None
    }
}
struct State {
    source: Arc<Playback>,
    ring: Ring,
    rate: u32,
    len: u32,
    cursor: AtomicU64,
    range: AtomicU64,
    sequence: AtomicU64,
    playing: Arc<AtomicBool>,
    stopped: AtomicBool,
    status: AtomicU8,
    end_epoch: AtomicU32,
    error: Arc<Mutex<Option<String>>>,
}
impl State {
    fn snapshot(&self) -> Option<(u64, u32, u32)> {
        let sequence = self.sequence.load(Ordering::Acquire);
        if sequence & 1 != 0 {
            return None;
        }
        let cursor = self.cursor.load(Ordering::Acquire);
        let range = self.range.load(Ordering::Acquire);
        (self.sequence.load(Ordering::Acquire) == sequence).then_some((
            cursor,
            range as u32,
            (range >> 32) as u32,
        ))
    }
    fn configure(&self, start: u32, end: u32, position: u32) {
        let epoch = (self.cursor.load(Ordering::Acquire) >> 32) as u32;
        let Some(epoch) = epoch.checked_add(1) else {
            self.fail("Playback seek epoch exhausted".into());
            return;
        };
        self.sequence.fetch_add(1, Ordering::AcqRel);
        self.range
            .store((u64::from(end) << 32) | u64::from(start), Ordering::Release);
        self.cursor.store(
            (u64::from(epoch) << 32) | u64::from(position),
            Ordering::Release,
        );
        self.status.store(1, Ordering::Release);
        self.end_epoch.store(0, Ordering::Release);
        self.sequence.fetch_add(1, Ordering::Release);
        self.source.prefetch(
            (u64::from(position) * u64::from(SAMPLE_RATE) / u64::from(self.rate)) as usize,
        );
        self.source.notify();
    }
    fn fail(&self, error: String) {
        self.playing.store(false, Ordering::Release);
        self.status.store(4, Ordering::Release);
        if let Ok(mut destination) = self.error.lock() {
            *destination = Some(error);
        }
    }
}
pub(super) struct Live {
    state: Arc<State>,
    thread: Option<JoinHandle<()>>,
}
impl Drop for Live {
    fn drop(&mut self) {
        self.state.stopped.store(true, Ordering::Release);
        self.state.source.notify();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Live {
    pub(super) fn range_seconds(&self) -> Option<(f64, f64)> {
        let (_, start, end) = self.state.snapshot()?;
        (start < end && end <= self.state.len).then_some((
            f64::from(start) / f64::from(self.state.rate),
            f64::from(end) / f64::from(self.state.rate),
        ))
    }
    pub(super) fn position(&self) -> f64 {
        (self.state.cursor.load(Ordering::Acquire) as u32) as f64 / f64::from(self.state.rate)
    }
    pub(super) fn seek(&self, seconds: f64) {
        let position = if seconds.is_finite() {
            (seconds.max(0.0) * f64::from(self.state.rate)).min(f64::from(self.state.len)) as u32
        } else {
            0
        };
        if let Some((_, start, end)) = self.state.snapshot() {
            self.state.configure(start, end, position);
        }
    }
    pub(super) fn full_range(&self) {
        self.state.configure(
            0,
            self.state.len,
            self.state.cursor.load(Ordering::Acquire) as u32,
        );
    }
    pub(super) fn set_range(&self, start: f64, end: f64) -> Result<(), String> {
        if !start.is_finite()
            || !end.is_finite()
            || start < 0.0
            || start >= end
            || end > self.state.source.duration() + 1.0 / f64::from(SAMPLE_RATE)
        {
            return Err("Playback range is outside the project".into());
        }
        let first = (start * f64::from(self.state.rate)).floor() as u32;
        let last = ((end * f64::from(self.state.rate)).ceil() as u32).min(self.state.len);
        if first >= last {
            return Err("Playback range has no samples".into());
        }
        self.state.configure(first, last, first);
        Ok(())
    }
    pub(super) fn resume(&self) {
        let Some((cursor, start, end)) = self.state.snapshot() else {
            return;
        };
        if start >= end {
            return;
        }
        let position = cursor as u32;
        if position < start || position >= end {
            self.state.configure(start, end, start);
        }
        self.state.playing.store(true, Ordering::Release);
    }
}

fn device_window(state: &State, first: u32, count: usize) -> Result<Option<Vec<f32>>, String> {
    let total = state.source.plan.samples;
    if state.rate == SAMPLE_RATE {
        return state
            .source
            .mix_window(first as usize, (first as usize + count).min(total));
    }
    let ratio = f64::from(SAMPLE_RATE) / f64::from(state.rate);
    let left = ((f64::from(first) * ratio).floor() as i64 - 31).max(0) as usize;
    let right =
        (((f64::from(first) + count as f64 - 1.0) * ratio).floor() as i64 + 33).max(0) as usize;
    let right = right.min(total);
    let Some(input) = state.source.mix_window(left.min(total), right)? else {
        return Ok(None);
    };
    let cutoff = (f64::from(state.rate) / f64::from(SAMPLE_RATE)).min(1.0) * 0.95;
    let mut output = Vec::with_capacity(count);
    for index in 0..count {
        let at = (f64::from(first) + index as f64) * ratio;
        let center = at.floor() as i64;
        let mut sum = 0.0;
        let mut weight = 0.0;
        for j in center - 31..=center + 32 {
            let delta = at - j as f64;
            let window = 0.42
                + 0.5 * (std::f64::consts::PI * delta / 32.0).cos()
                + 0.08 * (std::f64::consts::TAU * delta / 32.0).cos();
            let z = std::f64::consts::PI * cutoff * delta;
            let w = if z.abs() < 1e-12 {
                cutoff
            } else {
                cutoff * z.sin() / z
            } * window;
            if j >= 0 && (j as usize) < total {
                sum += f64::from(input[j as usize - left]) * w;
                weight += w;
            }
        }
        output.push(if weight.abs() > 1e-12 {
            (sum / weight) as f32
        } else {
            0.0
        });
    }
    Ok(Some(output))
}

fn feed(state: Arc<State>) {
    let mut epoch = 0;
    let mut next = 0u32;
    while !state.stopped.load(Ordering::Acquire) && !state.source.cancelled() {
        let Some((cursor, _, end)) = state.snapshot() else {
            state.source.wait();
            continue;
        };
        let current_epoch = (cursor >> 32) as u32;
        if current_epoch != epoch {
            epoch = current_epoch;
            next = cursor as u32;
        }
        if let Some(error) = state.source.failure() {
            state.fail(error.into());
            break;
        }
        if next >= end {
            state.end_epoch.store(epoch, Ordering::Release);
            state.source.wait();
            continue;
        }
        let count = state.ring.free().min(QUANTUM).min((end - next) as usize);
        if count == 0 {
            state.source.wait();
            continue;
        }
        let values = match device_window(&state, next, count) {
            Ok(Some(values)) => values,
            Ok(None) => {
                state.source.wait();
                continue;
            }
            Err(error) => {
                state.fail(error);
                break;
            }
        };
        if state
            .snapshot()
            .is_none_or(|(cursor, _, _)| (cursor >> 32) as u32 != epoch)
        {
            continue;
        }
        for value in values {
            if !value.is_finite() {
                state.fail("Invalid resampled playback frame".into());
                return;
            }
            if !state.ring.push(epoch, next, value) {
                break;
            }
            next += 1;
        }
    }
}

impl Player {
    pub fn playback_generation(&self) -> Option<u64> {
        self.live.as_ref().map(|live| live.state.source.generation)
    }
    pub fn buffering(&self) -> bool {
        self.live
            .as_ref()
            .is_some_and(|live| live.state.status.load(Ordering::Acquire) == 1)
    }
    pub fn ended(&self) -> bool {
        self.live
            .as_ref()
            .is_some_and(|live| live.state.status.load(Ordering::Acquire) == 3)
    }
    pub fn rewind_range(&self) {
        if let Some(live) = &self.live {
            if let Some((_, start, end)) = live.state.snapshot() {
                live.state.configure(start, end, start);
            }
        }
    }
    pub fn load_playback(
        &mut self,
        source: Arc<Playback>,
        output_id: &str,
        start: f64,
    ) -> Result<(), String> {
        self.stop();
        self.load_id = self
            .load_id
            .checked_add(1)
            .ok_or("Playback load identifier exhausted")?;
        let mut timing = LoadTiming::new(
            source.generation,
            self.load_id,
            SAMPLE_RATE,
            source.plan.samples,
        );
        let started = Instant::now();
        let host = cpal::default_host();
        let device = if output_id.is_empty() {
            host.default_output_device()
        } else {
            host.output_devices()
                .map_err(|e| e.to_string())?
                .find(|device| device.id().is_ok_and(|id| id.to_string() == output_id))
        }
        .ok_or("Selected audio output is unavailable")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        self.rate = supported.sample_rate();
        if self.rate == 0 {
            return Err("Audio device has an invalid sample rate".into());
        }
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        if config.channels == 0 {
            return Err("Audio device has no output channels".into());
        }
        self.len = ((source.plan.samples as u64 * u64::from(self.rate)
            + u64::from(SAMPLE_RATE) / 2)
            / u64::from(SAMPLE_RATE)) as usize;
        if self.len == 0 || self.len > u32::MAX as usize {
            return Err("Playback device length is outside limits".into());
        }
        timing.report.device_configuration_seconds = started.elapsed().as_secs_f64();
        timing.report.device_rate = self.rate;
        timing.report.channels = config.channels;
        timing.report.sample_format = format!("{format:?}");
        timing.report.resampled = self.rate != SAMPLE_RATE;
        timing.report.device_samples = self.len;
        let position = (start.max(0.0) * f64::from(self.rate)).min(self.len as f64) as u32;
        let state = Arc::new(State {
            source,
            ring: Ring::new(),
            rate: self.rate,
            len: self.len as u32,
            cursor: AtomicU64::new((1u64 << 32) | u64::from(position)),
            range: AtomicU64::new((self.len as u64) << 32),
            sequence: AtomicU64::new(0),
            playing: Arc::clone(&self.playing),
            stopped: AtomicBool::new(false),
            status: AtomicU8::new(1),
            end_epoch: AtomicU32::new(0),
            error: Arc::clone(&self.error),
        });
        let started = Instant::now();
        let stream = match format {
            cpal::SampleFormat::F32 => self.build_live::<f32>(
                &device,
                config,
                Arc::clone(&state),
                Arc::clone(&timing.callback),
            ),
            cpal::SampleFormat::F64 => self.build_live::<f64>(
                &device,
                config,
                Arc::clone(&state),
                Arc::clone(&timing.callback),
            ),
            cpal::SampleFormat::I16 => self.build_live::<i16>(
                &device,
                config,
                Arc::clone(&state),
                Arc::clone(&timing.callback),
            ),
            cpal::SampleFormat::I32 => self.build_live::<i32>(
                &device,
                config,
                Arc::clone(&state),
                Arc::clone(&timing.callback),
            ),
            cpal::SampleFormat::U16 => self.build_live::<u16>(
                &device,
                config,
                Arc::clone(&state),
                Arc::clone(&timing.callback),
            ),
            _ => Err("Selected device sample format is unsupported".into()),
        }?;
        timing.report.stream_build_seconds = started.elapsed().as_secs_f64();
        let feeder = Arc::clone(&state);
        let feeder_failure = Arc::clone(&state);
        let thread = std::thread::Builder::new()
            .name("roicepeak-playback".into())
            .spawn(move || {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| feed(feeder))).is_err()
                {
                    feeder_failure.fail("Playback feeder failed".into());
                }
            })
            .map_err(|e| e.to_string())?;
        self.live = Some(Live {
            state,
            thread: Some(thread),
        });
        let started = Instant::now();
        if let Err(error) = stream.play() {
            self.live = None;
            return Err(error.to_string());
        }
        timing.report.stream_start_seconds = started.elapsed().as_secs_f64();
        self.stream = Some(stream);
        timing.report.status = "streaming_loaded".into();
        timing.report.load_seconds = timing.callback.epoch.elapsed().as_secs_f64();
        self.timing = Some(timing);
        Ok(())
    }
    fn build_live<T: cpal::SizedSample + cpal::FromSample<f32>>(
        &self,
        device: &cpal::Device,
        config: cpal::StreamConfig,
        state: Arc<State>,
        timing: Arc<CallbackTiming>,
    ) -> Result<cpal::Stream, String> {
        let gain = Arc::clone(&self.gain);
        let failure = Arc::clone(&state);
        device
            .build_output_stream(
                config,
                move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let snapshot = state.snapshot();
                    let active = state.playing.load(Ordering::Acquire)
                        && !state.stopped.load(Ordering::Acquire)
                        && !state.source.cancelled();
                    let (initial, start, end) = snapshot.unwrap_or((0, 0, 0));
                    let epoch = (initial >> 32) as u32;
                    let mut position = initial as u32;
                    let mut consumed = false;
                    let mut buffering = false;
                    let gain = f32::from_bits(gain.load(Ordering::Relaxed));
                    for frame in out.chunks_mut(config.channels as usize) {
                        let value = if active
                            && state.playing.load(Ordering::Relaxed)
                            && (state.cursor.load(Ordering::Acquire) >> 32) as u32 == epoch
                            && snapshot.is_some()
                            && position >= start
                            && position < end
                        {
                            if let Some(value) = state.ring.pop(epoch, position) {
                                if !consumed {
                                    timing.consumed();
                                    consumed = true;
                                }
                                position += 1;
                                (value * gain).clamp(-1.0, 1.0)
                            } else {
                                buffering = true;
                                0.0
                            }
                        } else {
                            0.0
                        };
                        for (channel, sample) in frame.iter_mut().enumerate() {
                            *sample = T::from_sample(if channel < 2 { value } else { 0.0 });
                        }
                    }
                    if active
                        && state
                            .cursor
                            .compare_exchange(
                                initial,
                                (u64::from(epoch) << 32) | u64::from(position),
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .is_ok()
                    {
                        if position >= end && state.end_epoch.load(Ordering::Acquire) == epoch {
                            state.status.store(3, Ordering::Release);
                            state.playing.store(false, Ordering::Release);
                        } else {
                            state
                                .status
                                .store(if buffering { 1 } else { 2 }, Ordering::Release);
                        }
                    }
                },
                move |error| {
                    failure.playing.store(false, Ordering::Release);
                    failure.status.store(4, Ordering::Release);
                    if let Ok(mut destination) = failure.error.try_lock() {
                        *destination = Some(error.to_string());
                    }
                },
                Some(std::time::Duration::from_secs(3)),
            )
            .map_err(|e| e.to_string())
    }
}
