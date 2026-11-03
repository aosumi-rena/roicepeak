use crate::{Result, invalid};

pub(super) type Complex = [f32; 2];

pub(super) fn negative(value: f32) -> f32 {
    f32::from_bits(value.to_bits() ^ 0x8000_0000)
}

pub(super) fn finite(value: Complex) -> Result<Complex> {
    if value.iter().all(|component| component.is_finite()) {
        Ok(value)
    } else {
        Err(invalid("LF analytic complex value must remain finite"))
    }
}

pub(super) fn multiply(a: Complex, b: Complex) -> Complex {
    let real_positive = a[0] * b[0];
    let real_negative = negative(a[1]) * b[1];
    let imaginary_first = a[0] * b[1];
    let imaginary_second = a[1] * b[0];
    [
        real_positive + real_negative,
        imaginary_first + imaginary_second,
    ]
}

pub(super) fn divide(numerator: Complex, denominator: Complex) -> Result<Complex> {
    let [nr, ni] = finite(numerator)?;
    let [dr, di] = finite(denominator)?;
    let result = if dr.abs() > di.abs() {
        let ratio = di / dr;
        let product = di * ratio;
        let divisor = product + dr;
        if divisor == 0.0 || !divisor.is_finite() {
            return Err(invalid("LF analytic real-dominant divisor is unsupported"));
        }
        let real_product = ratio * ni;
        let imaginary_product = ratio * negative(nr);
        [
            (real_product + nr) / divisor,
            (imaginary_product + ni) / divisor,
        ]
    } else {
        if di == 0.0 {
            return Err(invalid("LF analytic complex divisor is singular"));
        }
        let ratio = dr / di;
        let product = dr * ratio;
        let divisor = product + di;
        if divisor == 0.0 || !divisor.is_finite() {
            return Err(invalid(
                "LF analytic imaginary-dominant divisor is unsupported",
            ));
        }
        let real_product = ratio * nr;
        let imaginary_product = ratio * ni;
        [
            (real_product + ni) / divisor,
            (imaginary_product + negative(nr)) / divisor,
        ]
    };
    finite(result)
}

pub(super) fn exponential(value: Complex, fused: bool) -> Result<Complex> {
    let [real, imaginary] = finite(value)?;
    let cosine = super::super::response_trig::cosine(imaginary, fused);
    let real_result = super::super::exp_scale::scaled_exp(real, cosine);
    let sine = super::super::response_trig::sine(imaginary, fused);
    let imaginary_result = super::super::exp_scale::scaled_exp(real, sine);
    finite([real_result, imaginary_result])
}
