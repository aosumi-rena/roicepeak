//! LF parameter interpolation and endpoint conversion.

use super::lf_table::SHAPE_TABLE;

pub(super) fn interpolate(shape: f32) -> [f32; 5] {
    let upper = SHAPE_TABLE.partition_point(|row| f32::from_bits(row[0]) <= shape);
    let lower = SHAPE_TABLE[upper.saturating_sub(1)];
    let mut output = std::array::from_fn(|i| f32::from_bits(lower[i + 1]));
    if upper == 0 || upper == SHAPE_TABLE.len() {
        return output;
    }
    let next = SHAPE_TABLE[upper];
    let weight =
        (shape - f32::from_bits(lower[0])) / (f32::from_bits(next[0]) - f32::from_bits(lower[0]));
    for i in 0..5 {
        let difference = f32::from_bits(next[i + 1]) - output[i];
        let adjustment = weight * difference;
        output[i] = adjustment + output[i];
    }
    output
}

pub(super) fn convert(mut coefficients: [f32; 5], fused: bool) -> [f32; 5] {
    coefficients[3] = super::response_exp::exponential(coefficients[3], fused);
    coefficients[4] = super::response_exp::exponential(coefficients[4], fused);
    coefficients
}

pub(super) fn coefficients(shape: f32, fused: bool) -> [f32; 5] {
    convert(interpolate(shape), fused)
}
