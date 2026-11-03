use super::{lf_atan, lf_math::Mode, lf_spectrum};
use crate::{Result, invalid};

fn mode(flag: Option<u8>) -> Result<Mode> {
    match flag {
        Some(flag) => {
            Mode::from_flag(flag).ok_or_else(|| invalid("LF math flag must be 0, 1 or 3"))
        }
        None => Ok(Mode::current()),
    }
}

pub struct LfArguments {
    pub cpu_flag: u8,
    pub values: Vec<f32>,
}

pub fn diagnostic_lf_arguments(pairs: &[[f32; 2]], flag: Option<u8>) -> Result<LfArguments> {
    if pairs.is_empty()
        || pairs.len() > 512
        || pairs.iter().flatten().any(|value| !value.is_finite())
    {
        return Err(invalid("LF argument requires 1..512 finite [y,x] pairs"));
    }
    let mode = mode(flag)?;
    let values: Vec<_> = pairs
        .iter()
        .map(|&[y, x]| lf_atan::argument(y, x, mode.alternate))
        .collect();
    if values.iter().any(|value| !value.is_finite()) {
        return Err(invalid("LF argument produced non-finite values"));
    }
    Ok(LfArguments {
        cpu_flag: mode.flag,
        values,
    })
}

pub struct LfSpectrum {
    pub cpu_flag: u8,
    pub prepared: [f32; 16],
    pub complex: Vec<[f32; 2]>,
    pub magnitudes: Vec<f32>,
    pub reference: f32,
    pub amplitudes: Vec<f32>,
    pub phases: Vec<f32>,
}

pub fn diagnostic_lf_spectrum(
    shape: f32,
    normalization_scale: f32,
    frequencies: &[f32],
    flag: Option<u8>,
) -> Result<LfSpectrum> {
    if frequencies.is_empty() || frequencies.len() > 480 {
        return Err(invalid("LF spectrum requires 1..480 frequencies"));
    }
    let mode = mode(flag)?;
    let prepared = lf_spectrum::Prepared::new(shape, normalization_scale, mode.fused)?;
    let complex = frequencies
        .iter()
        .map(|&frequency| prepared.complex(frequency))
        .collect::<Result<Vec<_>>>()?;
    let magnitudes = complex
        .iter()
        .map(|&value| lf_spectrum::magnitude(value))
        .collect::<Result<Vec<_>>>()?;
    let reference = lf_spectrum::magnitude(prepared.complex(200.0)?)?;
    let (amplitudes, phases) = prepared.amplitude_phase(frequencies, mode.alternate)?;
    Ok(LfSpectrum {
        cpu_flag: mode.flag,
        prepared: prepared.words(),
        complex,
        magnitudes,
        reference,
        amplitudes,
        phases,
    })
}
