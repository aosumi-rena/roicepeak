use serde::{Deserialize, de::DeserializeOwned};
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    input_bits: Vec<u32>,
    fused: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cepstral {
    parameter_bits: Vec<u32>,
}

fn read<T: DeserializeOwned>() -> Result<T, String> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(131_073)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 131_072 {
        return Err("Response diagnostic input exceeds 128 KiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn decode(words: Vec<u32>) -> Vec<f32> {
    words.into_iter().map(f32::from_bits).collect()
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|value| value.to_bits()).collect()
}

pub(super) fn run(args: &[OsString], endpoint: bool) -> Result<(), String> {
    if !args.is_empty() {
        return Err("Response diagnostics accept one JSON object on stdin".into());
    }
    let output = if endpoint {
        let input: Endpoint = read()?;
        let (values, fused) = roicepeak::vocoder::diagnostic_response_endpoint(
            &decode(input.input_bits),
            input.fused,
        )
        .map_err(|error| error.to_string())?;
        json!({"output_bits": bits(&values), "fused": fused})
    } else {
        let input: Cepstral = read()?;
        let boundary =
            roicepeak::vocoder::diagnostic_cepstral_response(&decode(input.parameter_bits))
                .map_err(|error| error.to_string())?;
        json!({
            "packed_input_bits": bits(&boundary.packed_input),
            "packed_transform_bits": bits(&boundary.packed_transform),
            "complex_exponential_included": false,
        })
    };
    let mut bytes = serde_json::to_vec(&output).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| error.to_string())
}

pub(super) fn phase(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-response-phase accepts one JSON object on stdin".into());
    }
    let input: Endpoint = read()?;
    let output =
        roicepeak::vocoder::diagnostic_response_phase(&decode(input.input_bits), input.fused)
            .map_err(|error| error.to_string())?;
    let mut bytes = serde_json::to_vec(&json!({
        "cosine_bits": bits(&output.cosine),
        "sine_bits": bits(&output.sine),
        "fused": output.fused,
    }))
    .map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| error.to_string())
}
