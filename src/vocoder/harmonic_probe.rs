use super::{LfCzt, lf_harmonics, lf_math::Mode};
use crate::{Result, invalid};

pub struct LfHarmonics {
    pub cpu_flag: u8,
    pub real: Vec<f32>,
    pub imaginary: Vec<f32>,
    pub normalized_amplitudes: Vec<f32>,
    pub phase_arguments: Vec<f32>,
    pub angular_step: f32,
    pub transformed: Option<LfCzt>,
}

pub fn diagnostic_lf_harmonics(
    sample_rate: f32,
    period: f32,
    f0: f32,
    fraction: f64,
    amplitudes: &[f32],
    phases: &[f32],
    flag: Option<u8>,
    transform: bool,
) -> Result<LfHarmonics> {
    let mode = match flag {
        Some(flag) => {
            Mode::from_flag(flag).ok_or_else(|| invalid("LF math flag must be 0, 1 or 3"))?
        }
        None => Mode::current(),
    };
    let packed = lf_harmonics::pack_fraction(
        sample_rate,
        f0,
        period,
        fraction,
        amplitudes,
        phases,
        mode.fused,
        true,
    )?;
    let transformed = if transform {
        Some(super::diagnostic_lf_generated_czt(
            &packed.real,
            &packed.imaginary,
            packed.angular_step,
            Some(mode.fused),
        )?)
    } else {
        None
    };
    Ok(LfHarmonics {
        cpu_flag: mode.flag,
        real: packed.real.to_vec(),
        imaginary: packed.imaginary.to_vec(),
        normalized_amplitudes: packed.normalized_amplitudes,
        phase_arguments: packed.phase_arguments.unwrap_or_default(),
        angular_step: packed.angular_step,
        transformed,
    })
}

pub fn diagnostic_lf_phase_step(before: f64, period: f32, f0: f32) -> Result<[f64; 3]> {
    let step = lf_harmonics::advance_phase(before, period, f0)?;
    Ok([step.before, step.after, step.fraction])
}
