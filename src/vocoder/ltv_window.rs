//! Float32 window construction and overlap boundaries for the LTV filter.
const LENGTH: usize = 2 * super::dsp::HOP;

fn number(bits: u64) -> f64 {
    f64::from_bits(bits)
}

fn cosine(value: f32, fused: bool) -> f32 {
    debug_assert!(value.is_finite() && (0.0..=std::f32::consts::TAU).contains(&value));
    let word = value.to_bits();
    let mut x = f64::from(value);
    if fused && word < 0x3c00_0000 {
        return if word < 0x3900_0000 {
            1.0
        } else {
            (-(x * 0.5)).mul_add(x, 1.0) as f32
        };
    }
    let mut quadrant = 0i32;
    if word > 0x3f49_0fdb {
        let inverse_half_pi = number(0x3fe4_5f30_6dc9_c883);
        quadrant = if fused {
            inverse_half_pi.mul_add(x, 0.5).trunc() as i32
        } else {
            (x * inverse_half_pi + 0.5).trunc() as i32
        };
        let n = f64::from(quadrant);
        let high = number(0x3ff9_21fb_5440_0000);
        let low = number(0x3dd0_b461_1a62_6331);
        let residual = if fused {
            (-n).mul_add(high, x)
        } else {
            x - n * high
        };
        let mut reduced = residual - n * low;
        if !fused {
            let original_exponent = ((x.to_bits() >> 52) & 2047) as i32;
            let reduced_exponent = ((reduced.to_bits() >> 52) & 2047) as i32;
            if original_exponent - reduced_exponent > 15 {
                let high_tail = n * number(0x3dd0_b461_1a60_0000);
                let a = residual - high_tail;
                let b = n * number(0x3ba3_198a_2e03_7073) - ((residual - a) - high_tail);
                reduced = a - b;
            }
            if reduced_exponent < 0x3f2 {
                let result = if reduced_exponent <= 0x3de {
                    if quadrant & 1 != 0 { reduced } else { 1.0 }
                } else if quadrant & 1 != 0 {
                    reduced - (number(0x3fc5_5555_5555_5555) * reduced) * (reduced * reduced)
                } else {
                    1.0 - (reduced * reduced) * 0.5
                };
                return if (quadrant + 1) & 2 != 0 {
                    -result as f32
                } else {
                    result as f32
                };
            }
        }
        x = reduced;
    }
    let square = x * x;
    let result = if quadrant & 1 != 0 {
        let a = number(0x3ec7_1de3_a556_c734);
        let b = number(0xbf2a_01a0_1a01_a01a);
        let c = number(0x3f81_1111_1111_1111);
        let d = number(0xbfc5_5555_5555_5555);
        if fused {
            let polynomial = a.mul_add(square, b).mul_add(square, c).mul_add(square, d);
            polynomial.mul_add(x * square, x)
        } else {
            x + ((a * square + b) * (square * square) + (c * square + d)) * (x * square)
        }
    } else {
        let a = number(0xbe92_7e4f_b778_9f5c);
        let b = number(0x3efa_01a0_1a01_a019);
        let c = number(0xbf56_c16c_16c1_6c16);
        let d = number(0x3fa5_5555_5555_5555);
        if fused {
            let polynomial = a.mul_add(square, b).mul_add(square, c).mul_add(square, d);
            polynomial.mul_add(square * square, 1.0 - square * 0.5)
        } else {
            (1.0 + (-0.5 * square))
                + ((a * square + b) * (square * square) + (c * square + d)) * (square * square)
        }
    };
    if (quadrant + 1) & 2 != 0 {
        -result as f32
    } else {
        result as f32
    }
}

pub(super) fn window() -> [f32; LENGTH] {
    window_with_mode(super::postfilter_log::fused_available())
}

pub(super) fn window_with_mode(fused: bool) -> [f32; LENGTH] {
    let sample = |index: usize| {
        let angle = (index as f32 / LENGTH as f32) * std::f32::consts::TAU;
        (1.0 - cosine(angle, fused)) * 0.5
    };
    let mut normalization = 0.0f32;
    for index in (0..LENGTH).step_by(super::dsp::HOP) {
        normalization += sample(index);
    }
    std::array::from_fn(|index| sample(index) / normalization)
}

pub(super) fn source_product(sample: f32, window: f32) -> f32 {
    sample * window
}

pub(super) fn overlap(output: &mut [f32], unscaled: &[f32]) {
    debug_assert!(unscaled.len() >= 32 && output.len() >= unscaled.len());
    let length = unscaled.len();
    let scale = 2.0 / length as f32;
    for (index, value) in unscaled.iter().enumerate() {
        let sample = *value * scale;
        let contribution = if index < 16 {
            ((index + 1) as f32 * 0.0625) * sample
        } else if index >= length - 16 {
            (((index as i32 - length as i32 + 17) as f32 * -0.0625) + 1.0) * sample
        } else {
            sample
        };
        output[index] += contribution;
    }
}
