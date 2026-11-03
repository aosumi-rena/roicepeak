mod polynomial;
mod reduction;

const QUARTER_PI: f64 = std::f64::consts::FRAC_PI_4;
const TINY: f64 = f64::from_bits(0x3e40_0000_0000_0000);
const SMALL: f64 = f64::from_bits(0x3f20_0000_0000_0000);

fn evaluate(value: f32, sine: bool, fused: bool) -> f32 {
    debug_assert!(value.is_finite() && value.abs() <= 361_000.0);
    let x = f64::from(value);
    let absolute = x.abs();
    let small = if fused && !sine {
        absolute <= QUARTER_PI
    } else {
        absolute < QUARTER_PI
    };
    if small {
        let output = if sine {
            if (fused && absolute < TINY) || (!fused && absolute <= TINY) {
                return value;
            }
            if fused && absolute < SMALL {
                (-((x * x) * x)).mul_add(-polynomial::S[0], x)
            } else {
                polynomial::small_sine(x, fused)
            }
        } else if absolute < TINY {
            1.0
        } else if absolute < SMALL {
            if fused {
                (-x).mul_add(x * 0.5, 1.0)
            } else {
                1.0 - (x * x) * 0.5
            }
        } else {
            polynomial::small_cosine(x, fused)
        };
        return output as f32;
    }
    let (r, tail, quadrant) = if fused {
        reduction::fused(absolute)
    } else {
        reduction::scalar(absolute)
    };
    let want_sine = (quadrant & 1 != 0) != sine;
    let output = if want_sine {
        polynomial::reduced_sine(r, tail, fused)
    } else {
        polynomial::reduced_cosine(r, tail, fused)
    };
    let negative = if sine {
        value.is_sign_negative() != (quadrant & 2 != 0)
    } else {
        (quadrant + 1) & 2 != 0
    };
    let output = if !negative {
        output
    } else if fused {
        f64::from_bits(output.to_bits() ^ (1 << 63))
    } else {
        0.0 - output
    };
    output as f32
}

pub(super) fn sine_cosine(value: f32, fused: bool) -> (f32, f32) {
    (evaluate(value, true, fused), evaluate(value, false, fused))
}
