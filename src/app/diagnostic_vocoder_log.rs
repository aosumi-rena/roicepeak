use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    words: Vec<u32>,
    #[serde(default)]
    cpu_flag: Option<u8>,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-vocoder-logarithm accepts one JSON object on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 65_536 {
        return Err("Vocoder logarithm input exceeds 64 KiB".into());
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let values: Vec<_> = input.words.into_iter().map(f32::from_bits).collect();
    let result = roicepeak::vocoder::diagnostic_vocoder_logarithms(&values, input.cpu_flag)
        .map_err(|error| error.to_string())?;
    let bits = |values: &[f32]| {
        values
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    };
    let mut output = serde_json::to_vec(&json!({"cpu_flag": result.cpu_flag,
        "embedded_bits": bits(&result.values), "host_bits": bits(&result.host_values)}))
    .map_err(|error| error.to_string())?;
    output.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&output)
        .map_err(|error| error.to_string())
}
