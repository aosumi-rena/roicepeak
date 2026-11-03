//! Device-delivery timing for our own PCM. No callback writes diagnostics or takes a lock.
use serde::Serialize;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

#[derive(Clone, Debug, Default, Serialize)]
pub struct OutputTimings {
    pub generation: u64,
    pub load_id: u64,
    pub status: String,
    pub validation_seconds: f64,
    pub device_configuration_seconds: f64,
    pub resample_seconds: f64,
    pub previous_stream_drop_seconds: f64,
    pub stream_build_seconds: f64,
    pub stream_start_seconds: f64,
    pub load_seconds: f64,
    pub source_rate: u32,
    pub device_rate: u32,
    pub channels: u16,
    pub sample_format: String,
    pub source_samples: usize,
    pub device_samples: usize,
    pub resampled: bool,
    /// First active callback that consumed a source frame, including a silent frame.
    /// Measures callback delivery, not microphone output.
    pub first_active_callback_after_load_seconds: Option<f64>,
    pub first_resume_after_load_seconds: Option<f64>,
    pub first_active_callback_after_resume_seconds: Option<f64>,
}
pub(super) struct CallbackTiming {
    pub(super) epoch: Instant,
    first_resume: AtomicU64,
    first_consumed: AtomicU64,
}
impl CallbackTiming {
    pub(super) fn new(epoch: Instant) -> Self {
        Self {
            epoch,
            first_resume: AtomicU64::new(0),
            first_consumed: AtomicU64::new(0),
        }
    }
    // Add one so a timestamp exactly at the epoch remains distinct from unset.
    fn stamp(&self) -> u64 {
        self.epoch
            .elapsed()
            .as_nanos()
            .min(u128::from(u64::MAX - 1)) as u64
            + 1
    }
    pub(super) fn resumed(&self) {
        if self.first_resume.load(Ordering::Relaxed) == 0 {
            let _ = self.first_resume.compare_exchange(
                0,
                self.stamp(),
                Ordering::Release,
                Ordering::Relaxed,
            );
        }
    }
    pub(super) fn consumed(&self) {
        if self.first_consumed.load(Ordering::Relaxed) == 0 {
            let _ = self.first_consumed.compare_exchange(
                0,
                self.stamp(),
                Ordering::Release,
                Ordering::Relaxed,
            );
        }
    }
    fn seconds(stamp: u64) -> Option<f64> {
        (stamp != 0).then(|| (stamp - 1) as f64 / 1_000_000_000.0)
    }
    pub(super) fn update(&self, report: &mut OutputTimings) -> u64 {
        let consumed = self.first_consumed.load(Ordering::Acquire);
        let resumed = self.first_resume.load(Ordering::Acquire);
        report.first_active_callback_after_load_seconds = Self::seconds(consumed);
        report.first_resume_after_load_seconds = Self::seconds(resumed);
        report.first_active_callback_after_resume_seconds =
            if consumed != 0 && resumed != 0 && consumed >= resumed {
                Some((consumed - resumed) as f64 / 1_000_000_000.0)
            } else {
                None
            };
        consumed
    }
}
pub(super) struct LoadTiming {
    pub(super) report: OutputTimings,
    pub(super) callback: std::sync::Arc<CallbackTiming>,
    initial_pending: bool,
    published_consumed: u64,
}
impl LoadTiming {
    pub(super) fn new(
        generation: u64,
        load_id: u64,
        source_rate: u32,
        source_samples: usize,
    ) -> Self {
        Self {
            report: OutputTimings {
                generation,
                load_id,
                source_rate,
                source_samples,
                status: "loading".into(),
                ..Default::default()
            },
            callback: std::sync::Arc::new(CallbackTiming::new(Instant::now())),
            initial_pending: true,
            published_consumed: 0,
        }
    }
    pub(super) fn poll(&mut self) -> Option<OutputTimings> {
        let consumed = self.callback.update(&mut self.report);
        if !self.initial_pending && consumed == self.published_consumed {
            return None;
        }
        self.initial_pending = false;
        self.published_consumed = consumed;
        Some(self.report.clone())
    }
}
