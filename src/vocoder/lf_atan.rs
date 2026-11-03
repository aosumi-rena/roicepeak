//! Finite F32 complex arguments with ordered F64 reduction.
mod table;

const LARGE_CUBIC: f64 = f64::from_bits(0x3fd5_5555_5555_0877);
const SMALL_C7: f64 = f64::from_bits(0x3fc2_4924_82bd_6be1);
const SMALL_C5: f64 = f64::from_bits(0x3fc9_9999_9996_43a3);
const SMALL_C3: f64 = f64::from_bits(0x3fd5_5555_5555_5538);
const SMALL_THRESHOLD: f64 = f64::from_bits(0x3f1a_36e2_eb1c_432d);

fn signed(value: f32, negative: bool) -> f32 {
    if negative { -value } else { value }
}

pub(super) fn argument(y: f32, x: f32, alternate: bool) -> f32 {
    if !y.is_finite() || !x.is_finite() {
        return f32::NAN;
    }
    let negative_y = y.is_sign_negative();
    let negative_x = x.is_sign_negative();
    let y = f64::from(y);
    let x = f64::from(x);
    let absolute_y = y.abs();
    let absolute_x = x.abs();
    if absolute_y == 0.0 {
        return if negative_x {
            signed(std::f32::consts::PI, negative_y)
        } else {
            y as f32
        };
    }
    if absolute_x == 0.0 {
        return signed(std::f32::consts::FRAC_PI_2, negative_y);
    }
    let difference = ((y.to_bits() >> 52) & 0x7ff) as i32 - ((x.to_bits() >> 52) & 0x7ff) as i32;
    if difference > 26 {
        return signed(std::f32::consts::FRAC_PI_2, negative_y);
    }
    if difference < -13 && !negative_x {
        if difference < -150 {
            return signed(0.0, negative_y);
        }
        if difference >= -126 {
            return (y / x) as f32;
        }
        let scaled = (f64::from_bits(0x4630_0000_0000_0000) * y) / x;
        return f64::from_bits(scaled.to_bits() - (100u64 << 52)) as f32;
    }
    if difference < -26 && negative_x {
        return signed(std::f32::consts::PI, negative_y);
    }

    let swapped = absolute_y > absolute_x;
    let (numerator, denominator) = if swapped {
        (absolute_x, absolute_y)
    } else {
        (absolute_y, absolute_x)
    };
    let ratio = numerator / denominator;
    let mut angle = if ratio > 0.0625 {
        let (index, reduced) = if alternate {
            let index = 256.0f64.mul_add(ratio, 0.5) as usize;
            let projected = denominator * index as f64;
            let scaled_denominator = denominator * 256.0;
            let top = numerator.mul_add(256.0, -projected);
            let bottom = numerator.mul_add(index as f64, scaled_denominator);
            (index, top / bottom)
        } else {
            let index = (ratio * 256.0 + 0.5) as usize;
            let scaled_numerator = numerator * 256.0;
            let projected = index as f64 * denominator;
            let top = scaled_numerator - projected;
            let projected_numerator = index as f64 * numerator;
            let scaled_denominator = denominator * 256.0;
            let bottom = projected_numerator + scaled_denominator;
            (index, top / bottom)
        };
        let head = reduced + table::ANGLES[index - 16];
        let square = reduced * reduced;
        let cube = square * reduced;
        if alternate {
            (-cube).mul_add(LARGE_CUBIC, head)
        } else {
            head - cube * LARGE_CUBIC
        }
    } else if ratio < SMALL_THRESHOLD {
        ratio
    } else {
        let square = ratio * ratio;
        if alternate {
            let p = (-SMALL_C7).mul_add(square, SMALL_C5);
            let q = (-p).mul_add(square, SMALL_C3);
            let cube = square * ratio;
            (-cube).mul_add(q, ratio)
        } else {
            let p = SMALL_C5 - square * SMALL_C7;
            let q = SMALL_C3 - p * square;
            let cube = square * ratio;
            ratio - q * cube
        }
    };
    if swapped {
        angle = std::f64::consts::FRAC_PI_2 - angle;
    }
    if negative_x {
        angle = std::f64::consts::PI - angle;
    }
    if negative_y {
        angle = f64::from_bits(angle.to_bits() ^ (1u64 << 63));
    }
    angle as f32
}
