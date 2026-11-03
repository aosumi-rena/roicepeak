//! Float32 gain, Hann window, and source overlap boundaries.

pub(super) const LENGTH: usize = 480;

pub(super) fn gain(log_gain: f32, fused: bool) -> f32 {
    super::response_exp::exponential(log_gain, fused)
}

pub(super) fn window(gain: f32, fused: bool) -> [f32; LENGTH] {
    std::array::from_fn(|index| {
        let fraction = index as f32 / LENGTH as f32;
        let angle = fraction * std::f32::consts::TAU;
        let cosine = super::response_trig::cosine(angle, fused);
        let difference = 1.0f32 - cosine;
        let hann = difference * 0.5f32;
        hann * gain
    })
}

pub(super) fn accumulate(source: f32, window: f32, existing: f32) -> f32 {
    let contribution = source * window;
    contribution + existing
}
