//! Float32 scaled exponential used when converting log-amplitude responses.

/// Compute `exp(value) * scale` with the reference scalar operation order.
/// Both inputs must be finite. Signed zero and gradual underflow are retained.
pub(super) fn scaled_exp(value: f32, scale: f32) -> f32 {
    debug_assert!(value.is_finite() && scale.is_finite());
    if scale == 0.0 {
        return scale;
    }
    if value < -229.0 {
        return 0.0f32.copysign(scale);
    }
    if value > 229.0 {
        return f32::INFINITY.copysign(scale);
    }
    let power = value * f32::from_bits(0x3fb8aa3b);
    let rounded = power + if power < 0.0 { -0.5 } else { 0.5 };
    let mut exponent = rounded as i32;
    let high = exponent as f32 * f32::from_bits(0x3f317200);
    let low = exponent as f32 * f32::from_bits(0x35bfbe8e);
    let residual = (value - high) - low;
    let fraction =
        if residual > -f32::from_bits(0x33000000) && residual < f32::from_bits(0x33000000) {
            scale
        } else {
            exponent -= 1;
            let square = residual * residual;
            let odd = (square + f32::from_bits(0x42705d55)) * residual;
            let even = square * f32::from_bits(0x41403e28) + f32::from_bits(0x42f05d55);
            let quotient = (odd + even) / (even - odd);
            (quotient * 2.0) * scale
        };
    scale_binary(fraction, exponent)
}

// Changing an IEEE exponent is exact until the result becomes subnormal. The
// final shift then rounds to nearest, ties to even, without double rounding.
fn scale_binary(value: f32, power: i32) -> f32 {
    let bits = value.to_bits();
    let sign = bits & 0x80000000;
    let mut exponent = ((bits >> 23) & 0xff) as i32;
    let mut significand = bits & 0x7fffff;
    if exponent == 0xff || (exponent == 0 && significand == 0) {
        return value;
    }
    if exponent == 0 {
        let shift = significand.leading_zeros() - 8;
        significand <<= shift;
        exponent = 1 - shift as i32;
    } else {
        significand |= 0x800000;
    }
    exponent += power;
    if exponent >= 255 {
        return f32::from_bits(sign | 0x7f800000);
    }
    if exponent > 0 {
        return f32::from_bits(sign | (exponent as u32) << 23 | (significand & 0x7fffff));
    }
    let shift = (1 - exponent) as u32;
    if shift > 24 {
        return f32::from_bits(sign);
    }
    let rounded = significand >> shift;
    let remainder = significand & ((1 << shift) - 1);
    let halfway = 1 << (shift - 1);
    let increment = remainder > halfway || (remainder == halfway && rounded & 1 != 0);
    f32::from_bits(sign | (rounded + u32::from(increment)))
}
