//! LF shape transfer with separate float32 and float64 stages.

pub(super) fn transfer(logit: f32, tension: f32, fused: bool) -> f32 {
    let exponential = super::response_exp::exponential(-logit, fused);
    let denominator = exponential + 1.0f32;
    let sigmoid = 1.0f32 / denominator;
    let scaled = sigmoid * 2.9f32;
    let base = scaled + 0.1f32;
    let base = f64::from(base);
    let distance = if tension > 0.0 {
        3.0f64 - base
    } else {
        base - 0.1f64
    };
    let adjustment = f64::from(tension) * distance;
    (base + adjustment) as f32
}
