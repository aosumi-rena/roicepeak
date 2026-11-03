use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Input {
    Argument {
        pairs: Vec<[u32; 2]>,
        #[serde(default)]
        cpu_flag: Option<u8>,
    },
    Spectrum {
        shape_bits: u32,
        normalization_scale_bits: u32,
        frequency_bits: Vec<u32>,
        #[serde(default)]
        cpu_flag: Option<u8>,
    },
    Harmonics {
        sample_rate_bits: u32,
        period_bits: u32,
        f0_bits: u32,
        phase_fraction_bits: u64,
        amplitude_bits: Vec<u32>,
        phase_bits: Vec<u32>,
        #[serde(default)]
        cpu_flag: Option<u8>,
        #[serde(default)]
        transform: bool,
    },
    PhaseStep {
        before_bits: u64,
        period_bits: u32,
        f0_bits: u32,
    },
    Source {
        voicing_bits: Vec<u32>,
        f0_bits: Vec<u32>,
        shape_bits: Vec<u32>,
        gain_bits: u32,
        #[serde(default)]
        cpu_flag: Option<u8>,
    },
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|value| value.to_bits()).collect()
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-lf-analytic accepts one JSON object on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(131_073)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 131_072 {
        return Err("LF analytic input exceeds 128 KiB".into());
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let result = (|| match input {
        Input::Argument { pairs, cpu_flag } => {
            let pairs: Vec<_> = pairs
                .into_iter()
                .map(|pair| pair.map(f32::from_bits))
                .collect();
            let output = roicepeak::vocoder::diagnostic_lf_arguments(&pairs, cpu_flag)?;
            Ok(json!({"cpu_flag": output.cpu_flag, "argument_bits": bits(&output.values)}))
        }
        Input::Spectrum {
            shape_bits,
            normalization_scale_bits,
            frequency_bits,
            cpu_flag,
        } => {
            let frequencies: Vec<_> = frequency_bits.into_iter().map(f32::from_bits).collect();
            let output = roicepeak::vocoder::diagnostic_lf_spectrum(
                f32::from_bits(shape_bits),
                f32::from_bits(normalization_scale_bits),
                &frequencies,
                cpu_flag,
            )?;
            let complex: Vec<_> = output
                .complex
                .iter()
                .map(|pair| pair.map(f32::to_bits))
                .collect();
            Ok(json!({
                "cpu_flag": output.cpu_flag,
                "prepared_bits": bits(&output.prepared),
                "complex_bits": complex,
                "magnitude_bits": bits(&output.magnitudes),
                "reference_bits": output.reference.to_bits(),
                "amplitude_bits": bits(&output.amplitudes),
                "phase_bits": bits(&output.phases),
            }))
        }
        Input::Harmonics {
            sample_rate_bits,
            period_bits,
            f0_bits,
            phase_fraction_bits,
            amplitude_bits,
            phase_bits,
            cpu_flag,
            transform,
        } => {
            let amplitudes: Vec<_> = amplitude_bits.into_iter().map(f32::from_bits).collect();
            let phases: Vec<_> = phase_bits.into_iter().map(f32::from_bits).collect();
            let output = roicepeak::vocoder::diagnostic_lf_harmonics(
                f32::from_bits(sample_rate_bits),
                f32::from_bits(period_bits),
                f32::from_bits(f0_bits),
                f64::from_bits(phase_fraction_bits),
                &amplitudes,
                &phases,
                cpu_flag,
                transform,
            )?;
            Ok(json!({
                "cpu_flag": output.cpu_flag,
                "real_bits": bits(&output.real),
                "imaginary_bits": bits(&output.imaginary),
                "normalized_amplitude_bits": bits(&output.normalized_amplitudes),
                "phase_argument_bits": bits(&output.phase_arguments),
                "angular_step_bits": output.angular_step.to_bits(),
                "output_real_bits": output.transformed.as_ref().map(|value| bits(&value.real)),
                "output_imaginary_bits": output.transformed.as_ref().map(|value| bits(&value.imaginary)),
            }))
        }
        Input::PhaseStep { before_bits, period_bits, f0_bits } => {
            let step = roicepeak::vocoder::diagnostic_lf_phase_step(
                f64::from_bits(before_bits), f32::from_bits(period_bits), f32::from_bits(f0_bits),
            )?;
            Ok(json!({"phase_bits": step.map(f64::to_bits)}))
        }
        Input::Source { voicing_bits, f0_bits, shape_bits, gain_bits, cpu_flag } => {
            let voicing: Vec<_> = voicing_bits.into_iter().map(f32::from_bits).collect();
            let f0: Vec<_> = f0_bits.into_iter().map(f32::from_bits).collect();
            let shapes: Vec<_> = shape_bits.into_iter().map(f32::from_bits).collect();
            let output = roicepeak::vocoder::diagnostic_lf_source(
                &voicing, &f0, &shapes, f32::from_bits(gain_bits), cpu_flag,
            )?;
            Ok(json!({"cpu_flag": output.cpu_flag, "sample_bits": bits(&output.values)}))
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
