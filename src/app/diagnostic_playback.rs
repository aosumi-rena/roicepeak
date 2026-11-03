use crate::app::{
    document::Project,
    render::{Config, Request, Worker},
};
use roicepeak::{tensor::Tensor, wave};
use serde_json::json;
use std::{
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

pub(super) fn render(
    config: Config,
    project: Project,
    user_dictionary: crate::app::dictionary::Snapshot,
    output: &Path,
) -> Result<(), String> {
    let mut worker = Worker::default();
    let started = Instant::now();
    worker.start_playback(
        Request {
            config: config.clone(),
            project: project.clone(),
            user_dictionary: user_dictionary.clone(),
        },
        0.0,
    )?;
    let source = super::wait(|| worker.poll_playback())?;
    let plan_seconds = started.elapsed().as_secs_f64();
    let first_end = source.plan.samples.min(128);
    let (first_ready, first_seconds) = loop {
        if source.mix_window(0, first_end)?.is_some() {
            break (source.ready_units(), started.elapsed().as_secs_f64());
        }
        if started.elapsed() > Duration::from_secs(1800) {
            return Err("Progressive playback exceeded 30 minutes".into());
        }
        source.wait();
    };
    let mixed = loop {
        if let Some(pcm) = source.mix_window(0, source.plan.samples)? {
            break pcm;
        }
        if started.elapsed() > Duration::from_secs(1800) {
            return Err("Progressive playback exceeded 30 minutes".into());
        }
        source.wait();
    };
    let progressive_seconds = started.elapsed().as_secs_f64();
    worker.submit(Request {
        config,
        project,
        user_dictionary,
    })?;
    let rendered = super::wait(|| worker.poll())?;
    worker.cancel();
    let stale_read_rejected = source.mix_window(0, first_end).is_err();
    let differences = mixed
        .iter()
        .zip(rendered.pcm.iter())
        .filter(|(actual, expected)| actual.to_bits() != expected.to_bits())
        .count();
    let same_length = mixed.len() == rendered.pcm.len();
    let tensor = Tensor::new(mixed.len(), 1, mixed).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    wave::write(&mut bytes, &tensor, rendered.rate, wave::Encoding::Float32)
        .map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    println!(
        "{}",
        json!({
            "samples":tensor.rows(), "units":source.total_units(),
            "ready_units_at_first_read":first_ready, "first_read_samples":first_end,
            "preparation_seconds":plan_seconds, "first_read_seconds":first_seconds,
            "progressive_completion_seconds":progressive_seconds,
            "whole_sentence_inference":true, "device_stream_opened":false,
            "same_length_as_eager":same_length, "different_pcm_words":differences,
            "bit_identical_to_eager":same_length && differences == 0,
            "cancelled_source":source.cancelled(), "stale_read_rejected":stale_read_rejected,
            "eager_profile":rendered.profile, "native_parity_qualified":false
        })
    );
    if !same_length || differences != 0 {
        return Err("Progressive playback and eager render PCM differ".into());
    }
    Ok(())
}
