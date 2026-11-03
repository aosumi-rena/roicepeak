use super::{lf_math::Mode, postfilter_log};
use crate::{Result, invalid};

pub struct VocoderLogarithms {
    pub cpu_flag: u8,
    pub values: Vec<f32>,
    pub host_values: Vec<f32>,
}

pub fn diagnostic_vocoder_logarithms(input: &[f32], flag: Option<u8>) -> Result<VocoderLogarithms> {
    if input.is_empty()
        || input.len() > 512
        || input
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
    {
        return Err(invalid(
            "vocoder logarithm requires 1..512 positive finite F32 values",
        ));
    }
    let mode = match flag {
        Some(flag) => {
            Mode::from_flag(flag).ok_or_else(|| invalid("math flag must be 0, 1 or 3"))?
        }
        None => Mode::current(),
    };
    Ok(VocoderLogarithms {
        cpu_flag: mode.flag,
        values: input
            .iter()
            .map(|value| postfilter_log::logarithm(*value, mode.fused))
            .collect(),
        host_values: input.iter().map(|value| value.ln()).collect(),
    })
}
