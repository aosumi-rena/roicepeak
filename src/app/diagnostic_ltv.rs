use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Cosine {
    #[default]
    Auto,
    Sse,
    AvxFma,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    cosine: Cosine,
    source_bits: Vec<u32>,
    unscaled_bits: Vec<u32>,
    overlap_bits: Vec<u32>,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-ltv-boundaries accepts one JSON object on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 65_536 {
        return Err("LTV boundary input exceeds 64 KiB".into());
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let requested = match input.cosine {
        Cosine::Auto => None,
        Cosine::Sse => Some(false),
        Cosine::AvxFma => Some(true),
    };
    let source = input
        .source_bits
        .into_iter()
        .map(f32::from_bits)
        .collect::<Vec<_>>();
    let unscaled = input
        .unscaled_bits
        .into_iter()
        .map(f32::from_bits)
        .collect::<Vec<_>>();
    let overlap = input
        .overlap_bits
        .into_iter()
        .map(f32::from_bits)
        .collect::<Vec<_>>();
    let result =
        roicepeak::vocoder::diagnostic_ltv_boundaries(&source, &unscaled, &overlap, requested)
            .map_err(|error| error.to_string())?;
    let bits = |values: &[f32]| {
        values
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    };
    let mut output = serde_json::to_vec(&json!({
        "cosine": if result.fused { "avx-fma" } else { "sse" },
        "window_bits": bits(&result.window),
        "windowed_source_bits": bits(&result.windowed),
        "overlap_bits": bits(&result.overlap),
        "native_fft_parity_qualified": false,
    }))
    .map_err(|error| error.to_string())?;
    output.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&output)
        .map_err(|error| error.to_string())
}
