//! Ordered forward Conv row composition; input padding rows are supplied explicitly.
#[allow(unsafe_code)]
#[path = "../matrix_storage/fused_product.rs"]
mod fused;

pub fn row(
    coefficients: &[f32],
    stride: usize,
    inputs: usize,
    taps: usize,
    queued: &[f32],
    queued_stride: usize,
    chunked: bool,
    output: &mut [f32],
    mut observe: Option<&mut dyn FnMut(&[f32])>,
) -> bool {
    if inputs == 0
        || taps == 0
        || queued_stride < inputs
        || inputs.checked_mul(taps).and_then(|x| x.checked_mul(stride)) != Some(coefficients.len())
        || taps.checked_mul(queued_stride) != Some(queued.len())
    {
        return false;
    }
    let mut start = 0;
    while start < inputs {
        let remaining = inputs - start;
        let count = if chunked && remaining >= 128 {
            64
        } else {
            remaining
        };
        for tap in 0..taps {
            if !fused::product_into(
                &coefficients
                    [(tap * inputs + start) * stride..(tap * inputs + start + count) * stride],
                stride,
                &queued[tap * queued_stride + start..tap * queued_stride + start + count],
                output,
                start != 0 || tap != 0,
            ) {
                return false;
            }
            if let Some(callback) = observe.as_mut() {
                callback(output)
            }
        }
        start += count;
    }
    true
}
