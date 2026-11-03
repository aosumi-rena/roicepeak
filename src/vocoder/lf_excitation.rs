use super::{dsp::HOP, lf_czt, lf_harmonics, lf_math::Mode, lf_spectrum, lf_window};
use crate::{Result, graph::Budget, invalid, tensor::Tensor};

const PERIOD: f32 = 0.005;

pub(super) fn excitation(
    physical: &Tensor,
    shape: &Tensor,
    gain: f32,
    budget: &mut Budget,
) -> Result<Tensor> {
    with_mode(physical, shape, gain, budget, Mode::current())
}

pub(super) fn with_mode(
    physical: &Tensor,
    shape: &Tensor,
    gain: f32,
    budget: &mut Budget,
    mode: Mode,
) -> Result<Tensor> {
    if physical.cols() != 72
        || shape.cols() != 1
        || shape.rows() != physical.rows()
        || !gain.is_finite()
    {
        return Err(invalid("excitation dimensions or gain"));
    }
    let frames = physical.rows();
    let count = frames
        .checked_mul(HOP)
        .ok_or_else(|| invalid("excitation length overflow"))?;
    budget.spend(frames.saturating_mul(150_000), count)?;
    let window = lf_window::window(gain, mode.fused);
    let sample_rate = HOP as f32 / PERIOD;
    let mut out = vec![0.0f32; count + HOP];
    let mut phase = 0.0;
    let mut frequencies = Vec::with_capacity(lf_harmonics::MAX_HARMONICS);
    let mut workspace = [0.0; lf_czt::WORDS];
    let mut transform: Option<(u32, lf_czt::Transform)> = None;
    for t in 0..frames {
        let f0 = physical.row(t)[1];
        let step = lf_harmonics::advance_phase(phase, PERIOD, f0)?;
        phase = step.after;
        if physical.row(t)[0] < 0.5 {
            continue;
        }
        let harmonics = lf_harmonics::harmonic_count(sample_rate, f0)?;
        if harmonics == 0 {
            continue;
        }
        frequencies.clear();
        frequencies.extend((1..=harmonics).map(|harmonic| harmonic as f32 * 100.0));
        let spectrum = lf_spectrum::Prepared::new(shape.row(t)[0], 100.0, mode.fused)?;
        let (amplitudes, phases) = spectrum.amplitude_phase(&frequencies, mode.alternate)?;
        let packed = lf_harmonics::pack_fraction(
            sample_rate,
            f0,
            PERIOD,
            step.fraction,
            &amplitudes,
            &phases,
            mode.fused,
            false,
        )?;
        if transform.as_ref().map(|(key, _)| *key) != Some(packed.angular_step.to_bits()) {
            transform = Some((
                packed.angular_step.to_bits(),
                lf_czt::Transform::from_angular_step(packed.angular_step, mode.fused)?,
            ));
        }
        let transform = &transform.as_ref().expect("LF transform was prepared").1;
        let (source, _) = transform.apply(&packed.real, &packed.imaginary, &mut workspace, None);
        for (i, source) in source.into_iter().enumerate() {
            let at = t * HOP + i;
            let value = lf_window::accumulate(source, window[i], out[at]);
            if !value.is_finite() {
                return Err(invalid("LF excitation produced non-finite values"));
            }
            out[at] = value;
        }
    }
    out.copy_within(HOP.., 0);
    out.truncate(count);
    Tensor::new(count, 1, out)
}
