use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    input_bits: Vec<u32>,
    #[serde(default)]
    inverse: bool,
    response_bits: Option<Vec<u32>>,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-real-fft accepts one JSON object on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 65_536 {
        return Err("Real FFT input exceeds 64 KiB".into());
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let decode = |bits: Vec<u32>| -> Result<Vec<f32>, String> {
        if bits.len() != 1024 {
            return Err("Real FFT requires exactly 1024 words per array".into());
        }
        let values = bits.into_iter().map(f32::from_bits).collect::<Vec<_>>();
        if values.iter().any(|value| !value.is_finite()) {
            return Err("Real FFT requires finite input words".into());
        }
        Ok(values)
    };
    let source = decode(input.input_bits)?;
    let bits = |values: &[f32]| {
        values
            .iter()
            .map(|value| value.to_bits())
            .collect::<Vec<_>>()
    };
    let result = if let Some(response) = input.response_bits {
        if input.inverse {
            return Err("response_bits requires a forward time-domain input".into());
        }
        let response = decode(response)?;
        let result = roicepeak::vocoder::diagnostic_ltv_fft(&source, &response)
            .map_err(|error| error.to_string())?;
        json!({
            "direction": "ltv",
            "spectrum_bits": bits(&result.spectrum),
            "product_bits": bits(&result.product),
            "inverse_bits": bits(&result.inverse),
            "input_windowed_and_padded": true,
            "inverse_scale": 512,
        })
    } else {
        let output = roicepeak::vocoder::diagnostic_real_fft(&source, input.inverse)
            .map_err(|error| error.to_string())?;
        json!({
            "direction": if input.inverse { "inverse" } else { "forward" },
            "output_bits": bits(&output),
            "inverse_scale": 512,
        })
    };
    let mut output = serde_json::to_vec(&result).map_err(|error| error.to_string())?;
    output.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&output)
        .map_err(|error| error.to_string())
}
