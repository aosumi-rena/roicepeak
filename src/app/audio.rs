//! Output-only playback. Rendering samples stay unchanged; device conversion is separate.
#[path = "audio_stream.rs"]
mod live;
#[path = "audio_timing.rs"]
mod timing;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
};
use std::time::Instant;
pub use timing::OutputTimings;
use timing::{CallbackTiming, LoadTiming};

#[derive(Clone)]
pub struct Output {
    pub id: String,
    pub name: String,
}
pub fn outputs() -> Result<Vec<Output>, String> {
    let host = cpal::default_host();
    let mut out = Vec::new();
    for d in host.output_devices().map_err(|e| e.to_string())? {
        if let (Ok(id), Ok(description)) = (d.id(), d.description()) {
            out.push(Output {
                id: id.to_string(),
                name: description.name().to_owned(),
            });
        }
    }
    Ok(out)
}
pub struct Player {
    live: Option<live::Live>,
    stream: Option<cpal::Stream>,
    cursor: Arc<AtomicUsize>,
    playing: Arc<AtomicBool>,
    repeating: Arc<AtomicBool>,
    range: Arc<AtomicU64>,
    gain: Arc<AtomicU32>,
    error: Arc<Mutex<Option<String>>>,
    rate: u32,
    len: usize,
    load_id: u64,
    timing: Option<LoadTiming>,
}
impl Default for Player {
    fn default() -> Self {
        Self {
            live: None,
            stream: None,
            cursor: Arc::new(AtomicUsize::new(0)),
            playing: Arc::new(AtomicBool::new(false)),
            repeating: Arc::new(AtomicBool::new(false)),
            range: Arc::new(AtomicU64::new(0)),
            gain: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            error: Arc::new(Mutex::new(None)),
            rate: 48000,
            len: 0,
            load_id: 0,
            timing: None,
        }
    }
}
impl Player {
    pub fn stop(&mut self) {
        self.pause();
        self.stream = None;
        self.live = None;
        self.cursor.store(0, Ordering::Relaxed);
        self.len = 0;
        self.range.store(0, Ordering::Relaxed);
    }
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
    pub fn loaded(&self) -> bool {
        self.stream.is_some()
    }
    pub fn pause(&self) {
        self.playing.store(false, Ordering::Relaxed);
    }
    pub fn resume(&self) {
        if let Some(live) = &self.live {
            live.resume();
            if let Some(timing) = &self.timing {
                timing.callback.resumed();
            }
            return;
        }
        let range = self.range.load(Ordering::Relaxed);
        let start = (range & 0xffff_ffff) as usize;
        let end = (range >> 32) as usize;
        if self.loaded() && start < end && end <= self.len {
            let position = self.cursor.load(Ordering::Relaxed);
            if position < start || position >= end {
                self.cursor.store(start, Ordering::Relaxed);
            }
            if let Some(timing) = &self.timing {
                timing.callback.resumed();
            }
            self.playing.store(true, Ordering::Release);
        }
    }
    pub fn seek(&self, seconds: f64) {
        if let Some(live) = &self.live {
            live.seek(seconds);
            return;
        }
        self.cursor.store(
            if seconds.is_finite() {
                ((seconds.max(0.0) * self.rate as f64) as usize).min(self.len)
            } else {
                0
            },
            Ordering::Relaxed,
        );
    }
    pub fn position(&self) -> f64 {
        if let Some(live) = &self.live {
            return live.position();
        }
        self.cursor.load(Ordering::Relaxed).min(self.len) as f64 / self.rate as f64
    }
    pub fn duration(&self) -> f64 {
        self.len as f64 / self.rate as f64
    }
    pub fn range_seconds(&self) -> Option<(f64, f64)> {
        if let Some(live) = &self.live {
            return live.range_seconds();
        }
        if !self.loaded() || self.rate == 0 {
            return None;
        }
        let range = self.range.load(Ordering::Relaxed);
        let start = (range & 0xffff_ffff) as usize;
        let end = (range >> 32) as usize;
        (start < end && end <= self.len).then_some((
            start as f64 / self.rate as f64,
            end as f64 / self.rate as f64,
        ))
    }
    pub fn repeat(&self, enabled: bool) {
        self.repeating.store(enabled, Ordering::Relaxed);
    }
    pub fn full_range(&self) {
        if let Some(live) = &self.live {
            live.full_range();
            return;
        }
        self.range.store((self.len as u64) << 32, Ordering::Relaxed);
    }
    pub fn set_range(&self, start: f64, end: f64) -> Result<(), String> {
        if let Some(live) = &self.live {
            return live.set_range(start, end);
        }
        if !self.loaded()
            || !start.is_finite()
            || !end.is_finite()
            || start < 0.0
            || start >= end
            || end > self.duration() + 1.0 / self.rate as f64
        {
            return Err("Playback range is outside rendered audio".into());
        }
        let start = (start * self.rate as f64).floor() as usize;
        let end = ((end * self.rate as f64).ceil() as usize).min(self.len);
        if start >= end {
            return Err("Playback range has no samples".into());
        }
        self.range
            .store(((end as u64) << 32) | start as u64, Ordering::Relaxed);
        self.cursor.store(start, Ordering::Relaxed);
        Ok(())
    }
    pub fn gain(&self, value: f32) {
        self.gain.store(
            if value.is_finite() {
                value.clamp(0.0, 2.0)
            } else {
                0.0
            }
            .to_bits(),
            Ordering::Relaxed,
        );
    }
    pub fn take_error(&self) -> Option<String> {
        self.error.try_lock().ok().and_then(|mut e| e.take())
    }
    pub fn load(
        &mut self,
        pcm: Arc<Vec<f32>>,
        source_rate: u32,
        output_id: &str,
    ) -> Result<(), String> {
        self.load_for_generation(pcm, source_rate, output_id, 0)
    }
    /// Associate device preparation and first consumed callback with this exact render.
    pub fn load_for_generation(
        &mut self,
        pcm: Arc<Vec<f32>>,
        source_rate: u32,
        output_id: &str,
        generation: u64,
    ) -> Result<(), String> {
        self.load_id = self
            .load_id
            .checked_add(1)
            .ok_or("Playback load identifier exhausted")?;
        let mut timing = LoadTiming::new(generation, self.load_id, source_rate, pcm.len());
        let result = self.load_measured(pcm, source_rate, output_id, &mut timing);
        timing.report.load_seconds = timing.callback.epoch.elapsed().as_secs_f64();
        timing.report.status = if result.is_ok() {
            "loaded"
        } else {
            "load_failed"
        }
        .into();
        self.timing = Some(timing);
        result
    }
    /// Returns the load report once, then the first consumed-frame update once if observed.
    /// Call on the UI/control thread. The real-time callback only updates atomics.
    pub fn poll_output_timing(&mut self) -> Option<OutputTimings> {
        self.timing.as_mut()?.poll()
    }
    fn load_measured(
        &mut self,
        pcm: Arc<Vec<f32>>,
        source_rate: u32,
        output_id: &str,
        timing: &mut LoadTiming,
    ) -> Result<(), String> {
        let started = Instant::now();
        let valid = source_rate != 0
            && !pcm.is_empty()
            && pcm.len() <= 48_000 * 60 * 60
            && !pcm.iter().any(|v| !v.is_finite());
        timing.report.validation_seconds = started.elapsed().as_secs_f64();
        if !valid {
            return Err("Invalid playback samples".into());
        }
        let started = Instant::now();
        let configuration = (|| {
            let host = cpal::default_host();
            let device = if output_id.is_empty() {
                host.default_output_device()
            } else {
                host.output_devices()
                    .map_err(|e| e.to_string())?
                    .find(|d| d.id().is_ok_and(|id| id.to_string() == output_id))
            }
            .ok_or("Selected audio output is unavailable")?;
            let supported = device.default_output_config().map_err(|e| e.to_string())?;
            Ok::<_, String>((device, supported))
        })();
        timing.report.device_configuration_seconds = started.elapsed().as_secs_f64();
        let (device, supported) = configuration?;
        let rate = supported.sample_rate();
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        timing.report.device_rate = rate;
        timing.report.channels = config.channels;
        timing.report.sample_format = format!("{format:?}");
        timing.report.resampled = rate != source_rate;
        let started = Instant::now();
        let samples = if rate == source_rate {
            Ok(pcm)
        } else {
            resample(&pcm, source_rate, rate).map(Arc::new)
        };
        timing.report.resample_seconds = started.elapsed().as_secs_f64();
        let samples = samples?;
        timing.report.device_samples = samples.len();
        let started = Instant::now();
        self.pause();
        self.stream = None;
        self.live = None;
        timing.report.previous_stream_drop_seconds = started.elapsed().as_secs_f64();
        self.cursor.store(0, Ordering::Relaxed);
        self.len = samples.len();
        self.rate = rate;
        self.full_range();
        let started = Instant::now();
        let callback = Arc::clone(&timing.callback);
        let stream = match format {
            cpal::SampleFormat::F32 => self.build::<f32>(&device, config, samples, callback),
            cpal::SampleFormat::F64 => self.build::<f64>(&device, config, samples, callback),
            cpal::SampleFormat::I16 => self.build::<i16>(&device, config, samples, callback),
            cpal::SampleFormat::I32 => self.build::<i32>(&device, config, samples, callback),
            cpal::SampleFormat::U16 => self.build::<u16>(&device, config, samples, callback),
            _ => Err("Selected device sample format is unsupported".into()),
        };
        timing.report.stream_build_seconds = started.elapsed().as_secs_f64();
        let stream = stream?;
        let started = Instant::now();
        let playing = stream.play().map_err(|e| e.to_string());
        timing.report.stream_start_seconds = started.elapsed().as_secs_f64();
        playing?;
        self.stream = Some(stream);
        Ok(())
    }
    fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
        &self,
        device: &cpal::Device,
        config: cpal::StreamConfig,
        samples: Arc<Vec<f32>>,
        callback_timing: Arc<CallbackTiming>,
    ) -> Result<cpal::Stream, String> {
        let cursor = self.cursor.clone();
        let playing = self.playing.clone();
        let repeating = self.repeating.clone();
        let range = self.range.clone();
        let gain = self.gain.clone();
        let error = self.error.clone();
        let playing_error = self.playing.clone();
        if config.channels == 0 {
            return Err("Audio device has no output channels".into());
        }
        device
            .build_output_stream(
                config,
                move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let start = cursor.load(Ordering::Relaxed);
                    let mut pos = start;
                    let active = playing.load(Ordering::Acquire);
                    let mut consumed = false;
                    let repeat = repeating.load(Ordering::Relaxed);
                    let range = range.load(Ordering::Relaxed);
                    let range_start = (range & 0xffff_ffff) as usize;
                    let range_end = ((range >> 32) as usize).min(samples.len());
                    let valid_range = range_start < range_end;
                    let gain = f32::from_bits(gain.load(Ordering::Relaxed));
                    for frame in out.chunks_mut(config.channels as usize) {
                        if active && valid_range && pos < range_start {
                            pos = range_start;
                        }
                        if active && valid_range && repeat && pos >= range_end {
                            pos = range_start;
                        }
                        let value = if active && valid_range && pos < range_end {
                            if !consumed {
                                callback_timing.consumed();
                                consumed = true;
                            }
                            let v = samples[pos] * gain;
                            pos += 1;
                            v.clamp(-1.0, 1.0)
                        } else {
                            0.0
                        };
                        for (channel, sample) in frame.iter_mut().enumerate() {
                            *sample = T::from_sample(if channel < 2 { value } else { 0.0 });
                        }
                    }
                    if cursor
                        .compare_exchange(start, pos, Ordering::Relaxed, Ordering::Relaxed)
                        .is_ok()
                        && (!valid_range || (!repeat && pos >= range_end))
                    {
                        playing.store(false, Ordering::Relaxed);
                    }
                },
                move |e| {
                    playing_error.store(false, Ordering::Relaxed);
                    if let Ok(mut target) = error.try_lock() {
                        *target = Some(e.to_string());
                    }
                },
                Some(std::time::Duration::from_secs(3)),
            )
            .map_err(|e| e.to_string())
    }
}
/// Windowed-sinc conversion for playback only. Raw synthesis/export uses 48 kHz samples.
pub(crate) fn resample(input: &[f32], source: u32, target: u32) -> Result<Vec<f32>, String> {
    if source == 0 || target == 0 {
        return Err("Invalid audio sample rate".into());
    }
    let length =
        ((input.len() as u64 * target as u64 + source as u64 / 2) / source as u64) as usize;
    if length > 48_000 * 60 * 60 {
        return Err("Playback resampling limit".into());
    }
    let ratio = source as f64 / target as f64;
    let cutoff = (target as f64 / source as f64).min(1.0) * 0.95;
    let mut out = Vec::with_capacity(length);
    for i in 0..length {
        let at = i as f64 * ratio;
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
            if j >= 0 && (j as usize) < input.len() {
                sum += input[j as usize] as f64 * w;
                weight += w;
            }
        }
        out.push(if weight.abs() > 1e-12 {
            (sum / weight) as f32
        } else {
            0.0
        });
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    #[test]
    fn playback_resampler_preserves_constant_and_length() {
        let input = vec![0.25; 480];
        let output = super::resample(&input, 48000, 44100).unwrap();
        assert_eq!(output.len(), 441);
        assert!(output.iter().all(|v| (*v - 0.25).abs() < 1e-6));
    }
}
