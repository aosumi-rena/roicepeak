//! Rounded phase trigonometry for finite filter parameters.
mod reduction;

fn number(bits: u64) -> f64 {
    f64::from_bits(bits)
}

fn polynomial(x: f64, sine: bool, fused: bool, sine_owner: bool) -> f64 {
    let square = x * x;
    if sine {
        let a = number(0x3ec7_1de3_a556_c734);
        let b = number(0xbf2a_01a0_1a01_a01a);
        let c = number(0x3f81_1111_1111_1111);
        let d = number(0xbfc5_5555_5555_5555);
        if fused {
            let q = a.mul_add(square, b).mul_add(square, c).mul_add(square, d);
            q.mul_add(x * square, x)
        } else {
            x + ((a * square + b) * (square * square) + (c * square + d)) * (x * square)
        }
    } else {
        let a = number(0xbe92_7e4f_b778_9f5c);
        let b = number(0x3efa_01a0_1a01_a019);
        let c = number(0xbf56_c16c_16c1_6c16);
        let d = number(0x3fa5_5555_5555_5555);
        if fused {
            let q = a.mul_add(square, b).mul_add(square, c).mul_add(square, d);
            let base = if sine_owner {
                square.mul_add(-0.5, 1.0)
            } else {
                1.0 - square * 0.5
            };
            q.mul_add(square * square, base)
        } else {
            (1.0 + (-0.5 * square))
                + ((a * square + b) * (square * square) + (c * square + d)) * (square * square)
        }
    }
}

fn finish(value: f64, negative: bool, fused: bool) -> f32 {
    let value = if negative {
        if fused {
            f64::from_bits(value.to_bits() ^ (1 << 63))
        } else {
            0.0 - value
        }
    } else {
        value
    };
    value as f32
}

fn evaluate(value: f32, sine: bool, fused: bool) -> f32 {
    debug_assert!(value.is_finite());
    let word = value.to_bits();
    let absolute = word & 0x7fff_ffff;
    if absolute <= 0x3f49_0fdb {
        let x = f64::from(value);
        let result = if sine {
            if absolute < 0x3900_0000 {
                return value;
            }
            if absolute < 0x3c00_0000 {
                let cube = (x * x) * x;
                if fused {
                    (-cube).mul_add(1.0 / 6.0, x)
                } else {
                    x - cube * (1.0 / 6.0)
                }
            } else {
                polynomial(x, true, fused, true)
            }
        } else if fused && absolute < 0x3c00_0000 {
            if absolute < 0x3900_0000 {
                1.0
            } else {
                (-(x * 0.5)).mul_add(x, 1.0)
            }
        } else {
            polynomial(x, false, fused, false)
        };
        return result as f32;
    }
    let x = f64::from(value.abs());
    let threshold = match (fused, sine) {
        (false, false) => 0x48f4_2400,
        (false, true) => 0x4b74_2400,
        (true, false) => 0x4f49_0fdb,
        (true, true) => 0x4b80_0456,
    };
    let (quadrant, reduced) = if absolute >= threshold {
        reduction::large(x, fused && !sine)
    } else {
        let inverse = number(0x3fe4_5f30_6dc9_c883);
        let high = number(0x3ff9_21fb_5440_0000);
        let low = number(0x3dd0_b461_1a62_6331);
        let count = if fused {
            inverse.mul_add(x, 0.5).trunc() as u32
        } else {
            (x * inverse + 0.5).trunc() as u32
        };
        let n = f64::from(count);
        let head = if fused {
            (-n).mul_add(high, x)
        } else {
            x - n * high
        };
        let mut reduced = head - n * low;
        if !fused {
            let original_exponent = ((x.to_bits() >> 52) & 2047) as i32;
            let reduced_exponent = ((reduced.to_bits() >> 52) & 2047) as i32;
            if original_exponent - reduced_exponent > 15 {
                let high_tail = n * number(0x3dd0_b461_1a60_0000);
                let a = head - high_tail;
                let b = n * number(0x3ba3_198a_2e03_7073) - ((head - a) - high_tail);
                reduced = a - b;
            }
            if reduced_exponent < 0x3f2 {
                let want_sine = if sine { count & 1 == 0 } else { count & 1 != 0 };
                let result = if reduced_exponent <= 0x3de {
                    if want_sine { reduced } else { 1.0 }
                } else if want_sine {
                    reduced - ((1.0 / 6.0) * reduced) * (reduced * reduced)
                } else {
                    1.0 - (reduced * reduced) * 0.5
                };
                let negative = if sine {
                    (count & 2 != 0) ^ (word >> 31 != 0)
                } else {
                    (count + 1) & 2 != 0
                };
                return finish(result, negative, false);
            }
        }
        (count, reduced)
    };
    let want_sine = if sine {
        quadrant & 1 == 0
    } else {
        quadrant & 1 != 0
    };
    let result = polynomial(reduced, want_sine, fused, sine);
    let negative = if sine {
        (quadrant & 2 != 0) ^ (word >> 31 != 0)
    } else {
        (quadrant + 1) & 2 != 0
    };
    finish(result, negative, fused)
}

pub(super) fn cosine(value: f32, fused: bool) -> f32 {
    evaluate(value, false, fused)
}

pub(super) fn sine(value: f32, fused: bool) -> f32 {
    evaluate(value, true, fused)
}
