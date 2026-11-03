//! Bounded diagnostics for ROICEPEAK's own worker; no source text is logged.
use super::Request;
use roicepeak::{
    Result as CoreResult,
    synthesis::{PreparedQuery, Render},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_EVENTS: usize = 16;
const MAX_RENDER_PROFILES: usize = 8;
const MAX_OUTPUT_LOADS: usize = 8;
const MAX_REPORT_BYTES: usize = 131_072;

pub fn diagnostics_path() -> Option<PathBuf> {
    crate::platform::config_dir().map(|root| root.join("diagnostics/last-render.json"))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CoreStages {
    pub encoding_seconds: f64,
    pub language_seconds: f64,
    pub prosody_seconds: f64,
    pub controls_seconds: f64,
    pub acoustic_seconds: f64,
    pub vocoder_and_gain_seconds: f64,
    pub total_seconds: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CoreCalls {
    pub attempted: usize,
    pub completed: usize,
    pub wall_seconds: f64,
    pub stages: CoreStages,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct OperatorStats {
    pub kind: String,
    pub input_channels: usize,
    pub maximum_output_channels: usize,
    pub calls: u64,
    pub failures: u64,
    pub input_elements: u64,
    pub output_elements: u64,
    pub inclusive_seconds: f64,
    pub exclusive_seconds: f64,
    pub inclusive_multiply_adds: u64,
    pub exclusive_multiply_adds: u64,
}

impl From<roicepeak::graph::profiling::OperatorTiming> for OperatorStats {
    fn from(value: roicepeak::graph::profiling::OperatorTiming) -> Self {
        Self {
            kind: value.kind.into(),
            input_channels: value.input_channels,
            maximum_output_channels: value.maximum_output_channels,
            calls: value.calls,
            failures: value.failures,
            input_elements: value.input_elements,
            output_elements: value.output_elements,
            inclusive_seconds: value.inclusive.as_secs_f64(),
            exclusive_seconds: value.exclusive.as_secs_f64(),
            inclusive_multiply_adds: value.inclusive_multiply_adds,
            exclusive_multiply_adds: value.exclusive_multiply_adds,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RenderProfile {
    pub process_id: u32,
    pub generation: u64,
    pub job_kind: String,
    pub status: String,
    pub submitted_unix_ms: u64,
    pub worker_started_unix_ms: Option<u64>,
    pub finished_unix_ms: Option<u64>,
    pub queue_seconds: f64,
    pub worker_seconds: f64,
    pub submit_to_finish_seconds: f64,
    pub dictionary_seconds: f64,
    pub dictionary_cache_hit: bool,
    pub voice_seconds: f64,
    pub voice_requests: usize,
    pub voice_cache_hits: usize,
    pub compiled_cache_retained_bytes: usize,
    pub compiled_cache_peak_retained_bytes: usize,
    pub compiled_cache_retained_entries: usize,
    pub compiled_cache_evictions: usize,
    pub compiled_cache_transient_loads: usize,
    pub voice_validation_seconds: f64,
    pub pcm_fast_path_hits: usize,
    pub prepared_metadata_cache_hits: usize,
    pub text_analysis_seconds: f64,
    pub analyzed_units: usize,
    pub analysis_cache_hits: usize,
    pub query_cache_hits: usize,
    pub query_cache_misses: usize,
    pub query_calls: CoreCalls,
    pub preview_controls_seconds: f64,
    pub prepared_units: usize,
    pub prepared_phones: usize,
    pub baseline_cache_hits: usize,
    pub baseline_cache_misses: usize,
    pub baseline_calls: CoreCalls,
    pub edited_calls: CoreCalls,
    pub blocks: usize,
    pub text_characters: usize,
    pub text_bytes: usize,
    pub request_sha256: String,
    pub narrators: Vec<String>,
    pub narrators_truncated: bool,
    pub output_samples: usize,
    pub sample_rate: u32,
    pub panicked: bool,
    pub output: Option<crate::app::audio::OutputTimings>,
    pub operators: Vec<OperatorStats>,
    pub operators_truncated: bool,
}

impl RenderProfile {
    pub(super) fn cache_state(&mut self, cache: &super::cache::Cache) {
        self.compiled_cache_retained_bytes = cache.retained_bytes();
        self.compiled_cache_peak_retained_bytes = self
            .compiled_cache_peak_retained_bytes
            .max(self.compiled_cache_retained_bytes);
        self.compiled_cache_retained_entries = cache.voices.len();
    }
    pub(super) fn voice_lease(&mut self, lease: &super::cache::VoiceLease) {
        self.voice_cache_hits += usize::from(lease.cache_hit);
        self.compiled_cache_retained_bytes = lease.retained_bytes;
        self.compiled_cache_peak_retained_bytes = self
            .compiled_cache_peak_retained_bytes
            .max(lease.retained_bytes);
        self.compiled_cache_retained_entries = lease.retained_entries;
        self.compiled_cache_evictions += lease.evicted;
        self.compiled_cache_transient_loads += usize::from(!lease.retained);
    }
    pub(super) fn new(request: &Request) -> Self {
        let mut text_characters = 0usize;
        let mut text_bytes = 0usize;
        let narrators: BTreeSet<_> = request
            .project
            .blocks
            .iter()
            .map(|block| {
                text_characters += block.text.chars().count();
                text_bytes += block.text.len();
                block.narrator.to_string()
            })
            .collect();
        // Hash the complete project snapshot so edits/repeated requests are
        // distinguishable without persisting their text or dictionary entries.
        let request_sha256 = serde_json::to_vec(&request.project)
            .map(|bytes| crate::app::hex::encode(&Sha256::digest(bytes)))
            .unwrap_or_else(|_| "unavailable".into());
        Self {
            process_id: std::process::id(),
            status: "submitted".into(),
            submitted_unix_ms: now_ms(),
            blocks: request.project.blocks.len(),
            text_characters,
            text_bytes,
            request_sha256,
            narrators_truncated: narrators.len() > 16
                || narrators.iter().any(|key| key.chars().count() > 256),
            narrators: narrators
                .into_iter()
                .take(16)
                .map(|key| key.chars().take(256).collect())
                .collect(),
            ..Self::default()
        }
    }

    pub(super) fn started(&mut self, queue: Duration) {
        self.status = "running".into();
        self.worker_started_unix_ms = Some(now_ms());
        self.queue_seconds = queue.as_secs_f64();
    }

    pub(super) fn finished(&mut self, status: &str, worker: Duration, total: Duration) {
        self.status = status.into();
        self.finished_unix_ms = Some(now_ms());
        self.worker_seconds = worker.as_secs_f64();
        self.submit_to_finish_seconds = total.as_secs_f64();
    }

    pub(super) fn query_call(&mut self, query: &CoreResult<PreparedQuery>, wall: Duration) {
        let calls = &mut self.query_calls;
        calls.attempted += 1;
        calls.wall_seconds += wall.as_secs_f64();
        let Ok(query) = query else {
            return;
        };
        calls.completed += 1;
        let Some(timing) = query.timings() else {
            return;
        };
        calls.stages.encoding_seconds += timing.encoding.as_secs_f64();
        calls.stages.language_seconds += timing.language.as_secs_f64();
        calls.stages.prosody_seconds += timing.prosody.as_secs_f64();
        calls.stages.total_seconds += timing.total.as_secs_f64();
    }

    pub(super) fn core_call(&mut self, edited: bool, render: &CoreResult<Render>, wall: Duration) {
        let calls = if edited {
            &mut self.edited_calls
        } else {
            &mut self.baseline_calls
        };
        calls.attempted += 1;
        calls.wall_seconds += wall.as_secs_f64();
        let Ok(render) = render else {
            return;
        };
        calls.completed += 1;
        let Some(timing) = &render.timings else {
            return;
        };
        calls.stages.encoding_seconds += timing.encoding.as_secs_f64();
        calls.stages.language_seconds += timing.language.as_secs_f64();
        calls.stages.prosody_seconds += timing.prosody.as_secs_f64();
        calls.stages.controls_seconds += timing.controls.as_secs_f64();
        calls.stages.acoustic_seconds += timing.acoustic.as_secs_f64();
        calls.stages.vocoder_and_gain_seconds += timing.vocoder_and_gain.as_secs_f64();
        calls.stages.total_seconds += timing.total.as_secs_f64();
    }
}

#[derive(Serialize)]
struct Event {
    generation: u64,
    job_kind: Option<String>,
    unix_ms: u64,
    status: String,
    request_sha256: Option<String>,
    queue_seconds: Option<f64>,
    worker_seconds: Option<f64>,
    baseline_calls: Option<usize>,
    edited_calls: Option<usize>,
}

#[derive(Default, Serialize)]
pub(super) struct Ledger {
    latest: Option<RenderProfile>,
    latest_render: Option<RenderProfile>,
    latest_prepare: Option<RenderProfile>,
    recent_renders: Vec<RenderProfile>,
    output_loads: Vec<crate::app::audio::OutputTimings>,
    events: Vec<Event>,
}

impl Ledger {
    pub(super) fn record(&mut self, profile: &RenderProfile) {
        self.record_memory(profile);
        self.persist();
    }

    /// Install lifecycle state without serializing or touching the filesystem.
    pub(super) fn record_memory(&mut self, profile: &RenderProfile) {
        let mut profile = profile.clone();
        // Lifecycle snapshots do not own device updates. Keep any timing that
        // has already been attached, including a retained load after eviction.
        let attached_output = self
            .recent_renders
            .iter()
            .chain(self.latest.iter())
            .chain(self.latest_render.iter())
            .find(|old| old.generation == profile.generation && old.output.is_some())
            .and_then(|old| old.output.clone())
            .or_else(|| {
                self.output_loads
                    .iter()
                    .filter(|timing| timing.generation == profile.generation)
                    .max_by_key(|timing| timing.load_id)
                    .cloned()
            });
        if attached_output.is_some() {
            profile.output = attached_output;
        }
        if self
            .latest
            .as_ref()
            .is_none_or(|old| old.generation <= profile.generation)
        {
            self.latest = Some(profile.clone());
        }
        if profile.job_kind == "prepare" {
            if self
                .latest_prepare
                .as_ref()
                .is_none_or(|old| old.generation <= profile.generation)
            {
                self.latest_prepare = Some(profile.clone());
            }
        } else {
            if self
                .latest_render
                .as_ref()
                .is_none_or(|old| old.generation <= profile.generation)
            {
                self.latest_render = Some(profile.clone());
            }
            self.recent_renders
                .retain(|old| old.generation != profile.generation);
            self.recent_renders.push(profile.clone());
            self.recent_renders.sort_by_key(|old| old.generation);
            if self.recent_renders.len() > MAX_RENDER_PROFILES {
                self.recent_renders.remove(0);
            }
        }
        self.event_memory(profile.generation, &profile.status);
    }

    pub(super) fn event(&mut self, generation: u64, status: &str) {
        self.event_memory(generation, status);
        self.persist();
    }

    fn event_memory(&mut self, generation: u64, status: &str) {
        if self.events.len() == MAX_EVENTS {
            self.events.remove(0);
        }
        let profile = self
            .latest
            .as_ref()
            .filter(|profile| profile.generation == generation)
            .or_else(|| {
                self.latest_render
                    .as_ref()
                    .filter(|profile| profile.generation == generation)
            })
            .or_else(|| {
                self.latest_prepare
                    .as_ref()
                    .filter(|profile| profile.generation == generation)
            })
            .or_else(|| {
                self.recent_renders
                    .iter()
                    .find(|profile| profile.generation == generation)
            });
        let event = Event {
            generation,
            job_kind: profile.map(|profile| profile.job_kind.clone()),
            unix_ms: now_ms(),
            status: status.into(),
            request_sha256: profile.map(|profile| profile.request_sha256.clone()),
            queue_seconds: profile.map(|profile| profile.queue_seconds),
            worker_seconds: profile.map(|profile| profile.worker_seconds),
            baseline_calls: profile.map(|profile| profile.baseline_calls.attempted),
            edited_calls: profile.map(|profile| profile.edited_calls.attempted),
        };
        self.events.push(event);
    }

    fn output_timing(&mut self, timing: crate::app::audio::OutputTimings) -> bool {
        // Generation zero is an unassociated device/settings load, not a render.
        if timing.generation == 0 {
            return false;
        }
        let generation = timing.generation;
        let mut attached = self
            .recent_renders
            .iter_mut()
            .find(|profile| profile.generation == generation)
            .map(|profile| {
                profile.output = Some(timing.clone());
            })
            .is_some();
        for profile in [&mut self.latest, &mut self.latest_render]
            .into_iter()
            .flatten()
        {
            if profile.generation == generation && profile.job_kind == "render" {
                profile.output = Some(timing.clone());
                attached = true;
            }
        }
        // Keep a bounded record even if an older render profile has been evicted;
        // the explicit generation remains unambiguous and is never reassigned.
        if let Some(previous) = self
            .output_loads
            .iter_mut()
            .find(|old| old.generation == generation && old.load_id == timing.load_id)
        {
            *previous = timing.clone();
        } else {
            if self.output_loads.len() == MAX_OUTPUT_LOADS {
                self.output_loads.remove(0);
            }
            self.output_loads.push(timing.clone());
        }
        self.event(
            generation,
            if timing.first_active_callback_after_load_seconds.is_some() {
                "output_first_consumed_callback"
            } else if timing.status == "loaded" {
                "output_loaded"
            } else {
                "output_load_failed"
            },
        );
        attached
    }

    pub(super) fn persist(&mut self) {
        let Some(path) = diagnostics_path() else {
            return;
        };
        let bytes = loop {
            let Ok(bytes) = serde_json::to_vec_pretty(&*self) else {
                return;
            };
            if bytes.len() <= MAX_REPORT_BYTES {
                break bytes;
            }
            // Keep the current worker reports intact; trim oldest history first.
            if !self.recent_renders.is_empty() {
                self.recent_renders.remove(0);
            } else if !self.output_loads.is_empty() {
                self.output_loads.remove(0);
            } else if !self.events.is_empty() {
                self.events.remove(0);
            } else {
                // Large narrator labels and many operator shapes may make the
                // latest reports exceed the cap even after history is gone.
                // Retain the hottest rows and explicitly mark the truncation.
                let candidate = [
                    &mut self.latest,
                    &mut self.latest_render,
                    &mut self.latest_prepare,
                ]
                .into_iter()
                .flatten()
                .filter(|profile| !profile.operators.is_empty())
                .max_by_key(|profile| profile.operators.len());
                if let Some(profile) = candidate {
                    profile.operators.pop();
                    profile.operators_truncated = true;
                } else {
                    return;
                }
            }
        };
        // Overwrite one fixed report; diagnostic I/O never fails synthesis.
        if fs::create_dir_all(path.parent().unwrap()).is_ok() {
            let _ = fs::write(path, bytes);
        }
    }
}

impl super::Worker {
    /// Persist on the UI/control thread after polling Player; never from CPAL.
    /// Returns whether the exact render generation is still in the bounded ledger.
    pub fn record_output_timing(&self, timing: crate::app::audio::OutputTimings) -> bool {
        self.shared
            .diagnostics
            .lock()
            .ok()
            .is_some_and(|mut ledger| ledger.output_timing(timing))
    }
}
