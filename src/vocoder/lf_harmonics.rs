//! LF harmonic scaling and transform input packing.

use crate::{Result, invalid};

pub const SIZE: usize = 480;
pub const MAX_HARMONICS: usize = SIZE;
const TAU: f32 = f32::from_bits(0x40c9_0fdb);
const NEGATIVE_TAU: f32 = f32::from_bits(0xc0c9_0fdb);
const SCALE: f32 = f32::from_bits(0x3dcc_cccd);

#[derive(Clone, Copy, Debug)]
pub struct PhaseStep {
    pub before: f64,
    pub after: f64,
    pub fraction: f64,
}

#[derive(Clone, Debug)]
pub struct Packed {
    pub real: [f32; SIZE],
    pub imaginary: [f32; SIZE],
    pub normalized_amplitudes: Vec<f32>,
    pub phase_arguments: Option<Vec<f32>>,
    pub angular_step: f32,
}

fn positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

/// Advance for every input frame, including frames without a voiced contribution.
pub fn advance_phase(before: f64, period: f32, f0: f32) -> Result<PhaseStep> {
    if !before.is_finite() || !positive(period) || !positive(f0) {
        return Err(invalid(
            "LF phase inputs must be finite with positive period and F0",
        ));
    }
    let increment = period * f0;
    let after = before + f64::from(increment);
    if !after.is_finite() {
        return Err(invalid("LF phase accumulator overflow"));
    }
    Ok(PhaseStep {
        before,
        after,
        fraction: after % 1.0,
    })
}

pub fn harmonic_count(sample_rate: f32, f0: f32) -> Result<usize> {
    if !positive(sample_rate) || !positive(f0) {
        return Err(invalid("LF sample rate and F0 must be finite and positive"));
    }
    let half_rate = sample_rate * 0.5;
    let quotient = half_rate / f0;
    if !quotient.is_finite() || quotient >= 2_147_483_648.0 {
        return Ok(0);
    }
    let count = quotient as i32;
    Ok(count.clamp(0, MAX_HARMONICS as i32) as usize)
}

pub fn pack_fraction(
    sample_rate: f32,
    f0: f32,
    period: f32,
    fraction: f64,
    amplitudes: &[f32],
    phases: &[f32],
    fused: bool,
    capture_arguments: bool,
) -> Result<Packed> {
    if !positive(period) || !fraction.is_finite() || fraction.abs() >= 1.0 {
        return Err(invalid("LF period or phase fraction is invalid"));
    }
    let count = harmonic_count(sample_rate, f0)?;
    if count == 0 || amplitudes.len() != count || phases.len() != count {
        return Err(invalid(
            "LF harmonic arrays must match the active harmonic count",
        ));
    }
    if !amplitudes
        .iter()
        .chain(phases)
        .all(|value| value.is_finite())
    {
        return Err(invalid("LF harmonic inputs must be finite"));
    }
    let angular_numerator = f0 * NEGATIVE_TAU;
    let angular_step = angular_numerator / sample_rate;
    let increment = period * f0;
    if !increment.is_finite() || !angular_step.is_finite() {
        return Err(invalid("LF harmonic scaling overflow"));
    }
    let fraction = fraction as f32;
    let negative_first = f32::from_bits(phases[0].to_bits() ^ 0x8000_0000);
    let mut packed = Packed {
        real: [0.0; SIZE],
        imaginary: [0.0; SIZE],
        normalized_amplitudes: Vec::with_capacity(count),
        phase_arguments: capture_arguments.then(|| Vec::with_capacity(count)),
        angular_step,
    };
    let count_float = count as f32;
    for (index, (&amplitude, &phase)) in amplitudes.iter().zip(phases).enumerate() {
        let amplitude = amplitude / count_float;
        let amplitude = amplitude / f0;
        let amplitude = amplitude * sample_rate;
        let amplitude = amplitude * SCALE;

        let harmonic = (index + 1) as f32;
        let reference = negative_first * harmonic;
        let core = reference + phase;
        let offset = fraction - increment;
        let offset = offset * harmonic;
        let offset = offset * TAU;
        let argument = offset + core;
        if !amplitude.is_finite() || !argument.is_finite() {
            return Err(invalid("LF harmonic packing overflow"));
        }
        let real = super::response_trig::cosine(argument, fused) * amplitude;
        let imaginary = super::response_trig::sine(argument, fused) * amplitude;
        packed.normalized_amplitudes.push(amplitude);
        if let Some(arguments) = &mut packed.phase_arguments {
            arguments.push(argument);
        }
        // The final harmonic still participates in scaling when its slot is discarded.
        if index + 1 < SIZE {
            packed.real[index + 1] = real;
            packed.imaginary[index + 1] = imaginary;
        }
    }
    Ok(packed)
}

pub fn pack_frame(
    sample_rate: f32,
    f0: f32,
    period: f32,
    phase_before: f64,
    amplitudes: &[f32],
    phases: &[f32],
    fused: bool,
    capture_arguments: bool,
) -> Result<(PhaseStep, Packed)> {
    let phase = advance_phase(phase_before, period, f0)?;
    let packed = pack_fraction(
        sample_rate,
        f0,
        period,
        phase.fraction,
        amplitudes,
        phases,
        fused,
        capture_arguments,
    )?;
    Ok((phase, packed))
}
