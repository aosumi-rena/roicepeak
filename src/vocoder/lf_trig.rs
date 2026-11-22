mod polynomial;
mod reduction;

const QUARTER_PI: f64 = std::f64::consts::FRAC_PI_4;
const TINY: f64 = f64::from_bits(0x3e40_0000_0000_0000);
const SMALL: f64 = f64::from_bits(0x3f20_0000_0000_0000);

pub(super) fn sine_cosine(value: f32, fused: bool) -> (f32, f32) {
    debug_assert!(value.is_finite() && value.abs() <= 361_000.0);
    let x = f64::from(value);
    let absolute = x.abs();
    if absolute <= QUARTER_PI {
        let sine = if absolute < TINY || (!fused && absolute == TINY) {
            value
        } else if fused && absolute < SMALL {
            (-((x * x) * x)).mul_add(-polynomial::S[0], x) as f32
        } else {
            polynomial::small_sine(x, fused) as f32
        };
        let cosine = if absolute < TINY {
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
        return (sine, cosine as f32);
    }
    let (r, tail, quadrant) = if fused {
        reduction::fused(absolute)
    } else {
        reduction::scalar(absolute)
    };
    let sine = polynomial::reduced_sine(r, tail, fused);
    let cosine = polynomial::reduced_cosine(r, tail, fused);
    let (sine, cosine) = if quadrant & 1 == 0 {
        (sine, cosine)
    } else {
        (cosine, sine)
    };
    let signed = |value: f64, negative: bool| {
        if !negative {
            value as f32
        } else if fused {
            f64::from_bits(value.to_bits() ^ (1 << 63)) as f32
        } else {
            (0.0 - value) as f32
        }
    };
    (
        signed(sine, value.is_sign_negative() != (quadrant & 2 != 0)),
        signed(cosine, (quadrant + 1) & 2 != 0),
    )
}
