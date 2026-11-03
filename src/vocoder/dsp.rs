use super::effects::{Effects, SpectrumKind};
use crate::{
    Result,
    graph::Budget,
    invalid,
    tensor::{MAX_ELEMENTS, Tensor},
};
pub const HOP: usize = 240;
const BANDS: usize = 8;

pub fn ltv(
    parameters: &Tensor,
    signal: &Tensor,
    spectral: bool,
    effect: Option<(&Effects, SpectrumKind, f32, f32)>,
    budget: &mut Budget,
) -> Result<Tensor> {
    let frames = parameters.rows();
    let length = frames
        .checked_mul(HOP)
        .ok_or_else(|| invalid("LTV frame overflow"))?;
    if signal.cols() != 1
        || signal.rows() != length
        || parameters.cols() == 0
        || parameters.cols() % 2 != 0
    {
        return Err(invalid("LTV dimensions"));
    }
    const SIZE: usize = 1024;
    const WINDOW: usize = 2 * HOP;
    const PADDING: usize = (SIZE - WINDOW) / 2;
    if spectral && parameters.cols() != 2 * (SIZE / 2 + 1) {
        return Err(invalid("spectral LTV channel width"));
    }
    if !spectral && parameters.cols() > SIZE {
        return Err(invalid("cepstral LTV channel width"));
    }
    budget.spend((frames + 1).saturating_mul(SIZE * 40), length)?;
    let mut out = vec![0.0f32; (frames + 1) * HOP + SIZE];
    let window = super::ltv_window::window();
    let mut buffer = [0.0f32; SIZE];
    let mut packed_response = [0.0f32; SIZE];
    let fused = super::postfilter_log::fused_available();
    for t in 0..=frames {
        if t < frames {
            packed_response = super::response::convert(parameters.row(t), spectral, fused)?;
            if let Some((effects, kind, tension, sample_rate)) = effect {
                budget.spend(SIZE * 128, SIZE)?;
                effects.apply(&mut packed_response, kind, tension, sample_rate)?;
            }
        } else {
            packed_response.fill(0.0);
        }
        buffer.fill(0.0);
        for i in 0..WINDOW {
            let at = t * HOP + i;
            if at >= HOP && at - HOP < length {
                buffer[PADDING + i] =
                    super::ltv_window::source_product(signal.row(at - HOP)[0], window[i]);
            }
        }
        super::real_fft::forward(&mut buffer);
        super::real_fft::multiply(&mut buffer, &packed_response);
        super::real_fft::inverse(&mut buffer);
        super::ltv_window::overlap(&mut out[t * HOP..t * HOP + SIZE], &buffer);
    }
    Tensor::new(length, 1, out[SIZE / 2..SIZE / 2 + length].to_vec())
}

pub fn upsample(input: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    const FACTOR: usize = HOP / BANDS;
    let rows = input
        .rows()
        .checked_mul(FACTOR)
        .ok_or_else(|| invalid("upsample overflow"))?;
    if input.rows() == 0 {
        return Tensor::zeros(0, input.cols());
    }
    budget.spend(
        rows.saturating_mul(input.cols()).saturating_mul(2),
        rows.saturating_mul(input.cols()),
    )?;
    let mut out = Tensor::zeros(rows, input.cols())?;
    for t in 0..FACTOR / 2 {
        out.row_mut(t).copy_from_slice(input.row(0));
    }
    for t in 0..input.rows() - 1 {
        for i in 0..FACTOR {
            let alpha = ((i as f64 + 0.5) / FACTOR as f64) as f32;
            for c in 0..input.cols() {
                out.row_mut(FACTOR / 2 + t * FACTOR + i)[c] =
                    (input.row(t + 1)[c] - input.row(t)[c]) * alpha + input.row(t)[c];
            }
        }
    }
    for t in rows - FACTOR / 2..rows {
        out.row_mut(t).copy_from_slice(input.row(input.rows() - 1));
    }
    Ok(out)
}
pub fn postfilter_noise(rows: usize, budget: &mut Budget) -> Result<Tensor> {
    postfilter_noise_mode(rows, super::postfilter_log::fused_available(), budget)
}
pub(super) fn postfilter_noise_mode(
    rows: usize,
    fused: bool,
    budget: &mut Budget,
) -> Result<Tensor> {
    budget.spend(rows.saturating_mul(BANDS * 64), rows.saturating_mul(BANDS))?;
    let mut state = 1u64;
    let count = rows
        .checked_mul(BANDS)
        .filter(|n| *n <= MAX_ELEMENTS)
        .ok_or_else(|| invalid("postfilter noise sample budget"))?;
    let mut values = Vec::with_capacity(count);
    while values.len() < count {
        state = state * 48271 % 2147483647;
        let u = (state as f32 - 1.0) * (2.0f32).powi(-31);
        let mut x = u + u - 1.0;
        state = state * 48271 % 2147483647;
        let u = (state as f32 - 1.0) * (2.0f32).powi(-31);
        let mut y = u + u - 1.0;
        let mut radius = x * x + y * y;
        if x == 0.0 || y == 0.0 || radius >= 1.0 {
            continue;
        }
        let log_radius = if radius > 0.0001 {
            super::postfilter_log::logarithm(radius, fused)
        } else {
            let exponent = ((x.abs().max(y.abs()).to_bits() >> 23) & 0xff) as i32 - 127;
            let factor = 2.0f32.powi(-exponent);
            x *= factor;
            y *= factor;
            radius = x * x + y * y;
            (exponent as f32 * f32::from_bits(0x3fb1_7218))
                + super::postfilter_log::logarithm(radius, fused)
        };
        let scale = (log_radius * -2.0 / radius).sqrt();
        values.push(x * scale + 0.0);
        values.push(y * scale + 0.0);
    }
    Tensor::new(rows, BANDS, values)
}
pub fn analysis(signal: &Tensor, taps: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    crate::pqmf::vocoder_analysis(signal, taps, budget)
}
pub fn synthesis(bands: &Tensor, taps: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    crate::pqmf::synthesis(bands, taps, budget)
}
