use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Input {
    ComplexFft {
        input_bits: Vec<u32>,
        inverse: bool,
        #[serde(default)]
        fused: Option<bool>,
    },
    Czt {
        real_bits: Vec<u32>,
        imaginary_bits: Vec<u32>,
        chirp_bits: Vec<u32>,
        #[serde(default)]
        fused: Option<bool>,
    },
    GeneratedCzt {
        real_bits: Vec<u32>,
        imaginary_bits: Vec<u32>,
        angular_step_bits: u32,
        #[serde(default)]
        fused: Option<bool>,
    },
    ChirpArguments {
        angular_step_bits: u32,
    },
}

fn floats(bits: Vec<u32>) -> Vec<f32> {
    bits.into_iter().map(f32::from_bits).collect()
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|value| value.to_bits()).collect()
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-lf-transform accepts one JSON object on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(131_073)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 131_072 {
        return Err("LF transform input exceeds 128 KiB".into());
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let result = (|| match input {
        Input::ComplexFft {
            input_bits,
            inverse,
            fused,
        } => {
            let (output, fused) =
                roicepeak::vocoder::diagnostic_lf_complex_fft(&floats(input_bits), inverse, fused)?;
            Ok(json!({"output_bits": bits(&output), "fused": fused}))
        }
        Input::Czt {
            real_bits,
            imaginary_bits,
            chirp_bits,
            fused,
        } => {
            let output = roicepeak::vocoder::diagnostic_lf_czt(
                &floats(real_bits),
                &floats(imaginary_bits),
                &floats(chirp_bits),
                fused,
            )?;
            Ok(json!({
                "real_bits": bits(&output.real),
                "imaginary_bits": bits(&output.imaginary),
                "chirp_bits": bits(&output.chirp),
                "packed_bits": bits(&output.packed),
                "input_fft_bits": bits(&output.input_fft),
                "kernel_fft_bits": bits(&output.kernel_fft),
                "product_bits": bits(&output.product),
                "inverse_bits": bits(&output.inverse),
                "fused": output.fused,
            }))
        }
        Input::GeneratedCzt {
            real_bits,
            imaginary_bits,
            angular_step_bits,
            fused,
        } => {
            let output = roicepeak::vocoder::diagnostic_lf_generated_czt(
                &floats(real_bits),
                &floats(imaginary_bits),
                f32::from_bits(angular_step_bits),
                fused,
            )?;
            Ok(json!({
                "real_bits": bits(&output.real),
                "imaginary_bits": bits(&output.imaginary),
                "chirp_bits": bits(&output.chirp),
                "packed_bits": bits(&output.packed),
                "input_fft_bits": bits(&output.input_fft),
                "kernel_fft_bits": bits(&output.kernel_fft),
                "product_bits": bits(&output.product),
                "inverse_bits": bits(&output.inverse),
                "fused": output.fused,
            }))
        }
        Input::ChirpArguments { angular_step_bits } => {
            let output = roicepeak::vocoder::diagnostic_lf_chirp_arguments(f32::from_bits(
                angular_step_bits,
            ))?;
            Ok(json!({"argument_bits": bits(&output)}))
        }
    })()
    .map_err(|error: roicepeak::Error| error.to_string())?;
    let mut bytes = serde_json::to_vec(&result).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| error.to_string())
}
