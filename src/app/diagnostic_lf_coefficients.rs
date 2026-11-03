use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    shape_bits: Vec<u32>,
    #[serde(default)]
    fused: Option<bool>,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-lf-coefficients accepts one JSON object on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(131_073)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 131_072 {
        return Err("LF coefficient diagnostic input exceeds 128 KiB".into());
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let shapes: Vec<_> = input.shape_bits.into_iter().map(f32::from_bits).collect();
    let output = roicepeak::vocoder::diagnostic_lf_coefficients(&shapes, input.fused)
        .map_err(|error| error.to_string())?;
    let bits = |rows: &[[f32; 5]]| {
        rows.iter()
            .map(|row| row.map(f32::to_bits))
            .collect::<Vec<_>>()
    };
    let value = json!({
        "raw_bits": bits(&output.raw),
        "output_bits": bits(&output.values),
        "fused": output.fused,
    });
    let mut bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| error.to_string())
}
