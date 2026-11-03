use super::{complex_fft, lf_czt, postfilter_log};
use crate::{Result, invalid};

fn finite_array<const N: usize>(input: &[f32]) -> Result<[f32; N]> {
    if input.len() != N || input.iter().any(|value| !value.is_finite()) {
        return Err(invalid(format!(
            "LF transform requires {N} finite F32 words"
        )));
    }
    input
        .try_into()
        .map_err(|_| invalid("LF transform dimensions"))
}

pub fn diagnostic_lf_complex_fft(
    input: &[f32],
    inverse: bool,
    fused: Option<bool>,
) -> Result<(Vec<f32>, bool)> {
    let mut output = finite_array::<2048>(input)?;
    let fused = fused.unwrap_or_else(postfilter_log::fused_available);
    complex_fft::transform(&mut output, inverse, fused);
    if output.iter().any(|value| !value.is_finite()) {
        return Err(invalid("LF complex FFT produced non-finite words"));
    }
    Ok((output.to_vec(), fused))
}

pub struct LfCzt {
    pub fused: bool,
    pub real: Vec<f32>,
    pub imaginary: Vec<f32>,
    pub chirp: Vec<f32>,
    pub packed: Vec<f32>,
    pub input_fft: Vec<f32>,
    pub kernel_fft: Vec<f32>,
    pub product: Vec<f32>,
    pub inverse: Vec<f32>,
}

pub fn diagnostic_lf_czt(
    real: &[f32],
    imaginary: &[f32],
    chirp: &[f32],
    fused: Option<bool>,
) -> Result<LfCzt> {
    let chirp = finite_array::<2048>(chirp)?;
    let fused = fused.unwrap_or_else(postfilter_log::fused_available);
    let transform = lf_czt::Transform::from_chirp(chirp, fused);
    czt_probe(real, imaginary, transform, fused)
}

pub fn diagnostic_lf_generated_czt(
    real: &[f32],
    imaginary: &[f32],
    angular_step: f32,
    fused: Option<bool>,
) -> Result<LfCzt> {
    let fused = fused.unwrap_or_else(postfilter_log::fused_available);
    let transform = lf_czt::Transform::from_angular_step(angular_step, fused)?;
    czt_probe(real, imaginary, transform, fused)
}

fn czt_probe(
    real: &[f32],
    imaginary: &[f32],
    transform: lf_czt::Transform,
    fused: bool,
) -> Result<LfCzt> {
    let real = finite_array::<480>(real)?;
    let imaginary = finite_array::<480>(imaginary)?;
    let chirp = transform.chirp().to_vec();
    let mut trace = lf_czt::Trace {
        packed: Vec::new(),
        input_fft: Vec::new(),
        kernel_fft: Vec::new(),
        product: Vec::new(),
        inverse: Vec::new(),
    };
    let (real, imaginary) = transform.apply(&real, &imaginary, &mut [0.0; 2048], Some(&mut trace));
    if real
        .iter()
        .chain(&imaginary)
        .chain(&trace.packed)
        .chain(&trace.input_fft)
        .chain(&trace.kernel_fft)
        .chain(&trace.product)
        .chain(&trace.inverse)
        .any(|value| !value.is_finite())
    {
        return Err(invalid("LF CZT produced non-finite words"));
    }
    Ok(LfCzt {
        fused,
        real: real.to_vec(),
        imaginary: imaginary.to_vec(),
        chirp,
        packed: trace.packed,
        input_fft: trace.input_fft,
        kernel_fft: trace.kernel_fft,
        product: trace.product,
        inverse: trace.inverse,
    })
}

pub fn diagnostic_lf_chirp_arguments(angular_step: f32) -> Result<Vec<f32>> {
    if !angular_step.is_finite() {
        return Err(invalid("LF chirp angular step must be finite"));
    }
    let values = lf_czt::chirp_arguments(angular_step);
    if values.iter().any(|value| !value.is_finite()) {
        return Err(invalid("LF chirp argument overflow"));
    }
    Ok(values.to_vec())
}
