//! Packed spectral and cepstral filter responses.
use crate::{Result, invalid};

pub(super) const SIZE: usize = 1024;

pub(super) fn cepstral_input(parameters: &[f32]) -> Result<[f32; SIZE]> {
    if parameters.len() < 2
        || parameters.len() > SIZE
        || parameters.len() % 2 != 0
        || parameters.iter().any(|value| !value.is_finite())
    {
        return Err(invalid(
            "cepstral response requires 2..1024 even finite parameters",
        ));
    }
    let mut output = [0.0; SIZE];
    let half = parameters.len() / 2;
    for i in 0..=half {
        let scaled = parameters[i] * 10.0;
        output[i] = scaled / (i + 1) as f32;
    }
    for i in 0..half - 1 {
        let scaled = parameters[half + 1 + i] * 10.0;
        output[SIZE + 1 - half + i] = scaled / (half - i) as f32;
    }
    if output.iter().any(|value| !value.is_finite()) {
        return Err(invalid("cepstral scaling produced non-finite values"));
    }
    Ok(output)
}

pub(super) fn convert(parameters: &[f32], spectral: bool, fused: bool) -> Result<[f32; SIZE]> {
    let mut output = if spectral {
        if parameters.len() != SIZE + 2 || parameters.iter().any(|value| !value.is_finite()) {
            return Err(invalid("spectral response requires 1026 finite parameters"));
        }
        [0.0; SIZE]
    } else {
        let mut output = cepstral_input(parameters)?;
        super::real_fft::forward(&mut output);
        output
    };
    if spectral {
        for i in 0..=SIZE / 2 {
            let phase = parameters[i + SIZE / 2 + 1];
            let real =
                super::response_exp(parameters[i], super::response_trig::cosine(phase, fused))?;
            let imaginary =
                super::response_exp(parameters[i], super::response_trig::sine(phase, fused))?;
            if i == 0 {
                output[0] = real;
            } else if i == SIZE / 2 {
                output[1] = real;
            } else {
                output[2 * i] = real;
                output[2 * i + 1] = -imaginary;
            }
        }
    } else {
        if output.iter().any(|value| !value.is_finite()) {
            return Err(invalid("cepstral transform produced non-finite values"));
        }
        output[0] = super::response_exp::exponential(output[0], fused);
        output[1] = super::response_exp::exponential(output[1], fused);
        for i in 1..SIZE / 2 {
            let log_amplitude = output[2 * i];
            let phase = output[2 * i + 1];
            output[2 * i] =
                super::response_exp(log_amplitude, super::response_trig::cosine(phase, fused))?;
            output[2 * i + 1] =
                super::response_exp(log_amplitude, super::response_trig::sine(phase, fused))?;
        }
    }
    if output.iter().any(|value| !value.is_finite()) {
        return Err(invalid("filter response produced non-finite values"));
    }
    Ok(output)
}
