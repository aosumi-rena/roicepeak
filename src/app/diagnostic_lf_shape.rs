use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    logit_bits: Vec<u32>,
    tension_bits: Vec<u32>,
    #[serde(default)]
    fused: Option<bool>,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-lf-shape accepts one JSON object on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(131_073)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 131_072 {
        return Err("LF shape diagnostic input exceeds 128 KiB".into());
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let logits: Vec<_> = input.logit_bits.into_iter().map(f32::from_bits).collect();
    let tensions: Vec<_> = input.tension_bits.into_iter().map(f32::from_bits).collect();
    let output = roicepeak::vocoder::diagnostic_lf_shape(&logits, &tensions, input.fused)
        .map_err(|error| error.to_string())?;
    let value = json!({"output_bits": output.values.iter().map(|value| value.to_bits()).collect::<Vec<_>>(), "fused": output.fused});
    let mut bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| error.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowInput {
    log_gain_bits: u32,
    source_bits: Vec<u32>,
    existing_bits: Vec<u32>,
    #[serde(default)]
    fused: Option<bool>,
}

pub(super) fn window(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-lf-window accepts one JSON object on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(131_073)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 131_072 {
        return Err("LF window diagnostic input exceeds 128 KiB".into());
    }
    let input: WindowInput = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let source: Vec<_> = input.source_bits.into_iter().map(f32::from_bits).collect();
    let existing: Vec<_> = input
        .existing_bits
        .into_iter()
        .map(f32::from_bits)
        .collect();
    let output = roicepeak::vocoder::diagnostic_lf_window(
        f32::from_bits(input.log_gain_bits),
        &source,
        &existing,
        input.fused,
    )
    .map_err(|error| error.to_string())?;
    let value = json!({"gain_bits": output.gain.to_bits(),
        "window_bits": output.window.iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
        "overlap_bits": output.overlap.iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
        "fused": output.fused});
    let mut bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| error.to_string())
}
