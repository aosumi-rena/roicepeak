//! Float32 scaled exponential used when converting log-amplitude responses.

/// Evaluate scaled exponentials while preserving signed zero and underflow.
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
    let (exponent, factor) = decompose(value);
    scale_binary(factor.map_or(scale, |factor| factor * scale), exponent)
}

pub(super) fn scaled_exp_pair(value: f32, scales: [f32; 2]) -> [f32; 2] {
    debug_assert!(value.is_finite() && scales.iter().all(|value| value.is_finite()));
    if !(-229.0..=229.0).contains(&value) {
        return scales.map(|scale| scaled_exp(value, scale));
    }
    let (exponent, factor) = decompose(value);
    scales.map(|scale| {
        if scale == 0.0 {
            scale
        } else {
            scale_binary(factor.map_or(scale, |factor| factor * scale), exponent)
        }
    })
}

fn decompose(value: f32) -> (i32, Option<f32>) {
    let power = value * f32::from_bits(0x3fb8aa3b);
    let rounded = power + if power < 0.0 { -0.5 } else { 0.5 };
    let mut exponent = rounded as i32;
    let high = exponent as f32 * f32::from_bits(0x3f317200);
    let low = exponent as f32 * f32::from_bits(0x35bfbe8e);
    let residual = (value - high) - low;
    let factor = if residual > -f32::from_bits(0x33000000) && residual < f32::from_bits(0x33000000)
    {
        None
    } else {
        exponent -= 1;
        let square = residual * residual;
        let odd = (square + f32::from_bits(0x42705d55)) * residual;
        let even = square * f32::from_bits(0x41403e28) + f32::from_bits(0x42f05d55);
        let quotient = (odd + even) / (even - odd);
        Some(quotient * 2.0)
    };
    (exponent, factor)
}

// Round subnormal results once, to nearest with ties to even.
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
