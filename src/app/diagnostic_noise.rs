use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    rows: Vec<usize>,
    #[serde(default)]
    logarithm: Logarithm,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
enum Logarithm {
    #[default]
    Auto,
    Sse,
    AvxFma,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-postfilter-noise accepts a JSON batch on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65_536 {
        return Err("Noise batch exceeds 64 KiB".into());
    }
    let batch: Batch = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if batch.rows.len() > 64 {
        return Err("Noise batch exceeds 64 cases".into());
    }
    let mut samples = 0usize;
    let mut results = Vec::with_capacity(batch.rows.len());
    let requested = match batch.logarithm {
        Logarithm::Auto => None,
        Logarithm::Sse => Some(false),
        Logarithm::AvxFma => Some(true),
    };
    let mut selected = None;
    let mut budget = roicepeak::graph::Budget::default();
    for rows in batch.rows {
        samples = rows
            .checked_mul(8)
            .and_then(|n| samples.checked_add(n))
            .filter(|n| *n <= 1_000_000)
            .ok_or("Noise batch sample budget")?;
        let (value, fused) =
            roicepeak::vocoder::diagnostic_postfilter_noise(rows, requested, &mut budget)
                .map_err(|e| e.to_string())?;
        selected = Some(if fused { "avx-fma" } else { "sse" });
        results.push(json!({"rows": value.rows(), "columns": value.cols(),
            "bits": value.values().iter().map(|v| v.to_bits()).collect::<Vec<_>>()}));
    }
    let mut data = serde_json::to_vec(&json!({"logarithm": selected, "results": results}))
        .map_err(|e| e.to_string())?;
    data.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&data)
        .map_err(|e| e.to_string())
}
