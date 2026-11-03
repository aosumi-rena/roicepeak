//! Ordered float32 analytic LF spectrum and normalized harmonic parameters.
mod complex;

use crate::{Result, invalid};
use complex::negative;

const PERIOD: f32 = f32::from_bits(0x3ba3_d70a);
const PI: f32 = f32::from_bits(0x4049_0fdb);
const TAU: f32 = f32::from_bits(0x40c9_0fdb);

pub(super) struct Prepared {
    te: f32,
    tp: f32,
    ta: f32,
    scale: f32,
    wp: f32,
    wp2: f32,
    alpha: f32,
    epsilon: f32,
    sine: f32,
    a: f32,
    b: f32,
    c: f32,
    e0: f32,
    opening_real: f32,
    remaining: f32,
    negative_epsilon_ta: f32,
    fused: bool,
}

impl Prepared {
    pub fn new(shape: f32, normalization_scale: f32, fused: bool) -> Result<Self> {
        if !shape.is_finite() || !normalization_scale.is_finite() || normalization_scale <= 0.0 {
            return Err(invalid(
                "LF analytic shape and positive normalization scale must be finite",
            ));
        }
        let [c0, c1, c2, epsilon, alpha] = super::lf_coefficients::coefficients(shape, fused);
        let te = c0 * PERIOD;
        let tp = c1 * PERIOD;
        let ta = c2 * PERIOD;
        let scale = normalization_scale * PERIOD;
        if tp <= 0.0 || scale <= 0.0 {
            return Err(invalid(
                "LF analytic period and frequency scale must be positive",
            ));
        }
        let wp = PI / tp;
        let angle = wp * te;
        let sine = super::response_trig::sine(angle, fused);
        let cosine = super::response_trig::cosine(angle, fused);
        let opening_real = te * alpha;
        let opening_exp = super::response_exp::exponential(opening_real, fused);
        let opening_product = opening_exp * sine;
        if opening_product == 0.0 || !opening_product.is_finite() {
            return Err(invalid("LF analytic opening normalization is unsupported"));
        }
        let e0 = -1.0 / opening_product;
        let a = ta * epsilon;
        let c = ((1.0 - f64::from(a)) * f64::from(epsilon)) as f32;
        let b_first = sine * alpha;
        let b_second = cosine * wp;
        let result = Self {
            te,
            tp,
            ta,
            scale,
            wp,
            wp2: wp * wp,
            alpha,
            epsilon,
            sine,
            a,
            b: b_first - b_second,
            c,
            e0,
            opening_real,
            remaining: PERIOD - te,
            negative_epsilon_ta: negative(epsilon) * ta,
            fused,
        };
        if !result.words().iter().all(|value| value.is_finite()) {
            return Err(invalid(
                "LF analytic prepared parameters must remain finite",
            ));
        }
        Ok(result)
    }

    /// te, tp, ta, scale, wp, wp², alpha, epsilon, sine, A, B, c, e0,
    /// opening exponent, remaining period, and negative epsilon times ta.
    pub fn words(&self) -> [f32; 16] {
        [
            self.te,
            self.tp,
            self.ta,
            self.scale,
            self.wp,
            self.wp2,
            self.alpha,
            self.epsilon,
            self.sine,
            self.a,
            self.b,
            self.c,
            self.e0,
            self.opening_real,
            self.remaining,
            self.negative_epsilon_ta,
        ]
    }

    pub fn complex(&self, frequency: f32) -> Result<[f32; 2]> {
        if !frequency.is_finite() || frequency <= 0.0 {
            return Err(invalid("LF analytic frequency must be positive and finite"));
        }
        let scaled_frequency = frequency.max(f32::from_bits(0x3dcc_cccd)) * TAU;
        let omega = scaled_frequency / self.scale;
        if !omega.is_finite() {
            return Err(invalid("LF analytic angular frequency must remain finite"));
        }
        let negative_omega = negative(omega);
        let alpha_square = self.alpha * self.alpha;
        let omega_square = omega * omega;
        let dr = (alpha_square - omega_square) - self.wp2;
        let di_first = self.alpha * negative_omega;
        let di_second = omega * self.alpha;
        let opening_coefficient = complex::divide([self.e0, 0.0], [dr, di_first - di_second])?;
        let argument = negative_omega * self.te;
        let exponent = complex::exponential([self.opening_real, argument], self.fused)?;
        let mut numerator = complex::multiply(exponent, [self.b, negative_omega * self.sine]);
        numerator[0] += self.wp;
        numerator[1] += 0.0;
        let opening = complex::multiply(opening_coefficient, numerator);

        let aw = self.a * omega;
        let dr_zero = self.epsilon * 0.0;
        let dr_product = negative(aw) * omega;
        let di_zero = omega * 0.0;
        let di_product = self.epsilon * aw;
        let numerator = complex::exponential([0.0, argument], self.fused)?;
        let ratio = complex::divide(numerator, [dr_zero + dr_product, di_zero + di_product])?;
        let q = if self.c < 1.0 {
            [0.0, omega * self.negative_epsilon_ta]
        } else {
            let [er, ei] =
                complex::exponential([0.0, self.remaining * negative_omega], self.fused)?;
            let real_first = self.c * er;
            let real_zero = ei * -0.0;
            let imaginary_first = self.c * ei;
            let imaginary_zero = er * 0.0;
            [
                self.c - (real_first + real_zero),
                (0.0 - (imaginary_first + imaginary_zero)) - aw,
            ]
        };
        let tail = complex::multiply(ratio, q);
        complex::finite([opening[0] + tail[0], opening[1] + tail[1]])
    }

    pub fn amplitude_phase(
        &self,
        frequencies: &[f32],
        alternate: bool,
    ) -> Result<(Vec<f32>, Vec<f32>)> {
        let mut amplitudes = Vec::with_capacity(frequencies.len());
        let mut phases = Vec::with_capacity(frequencies.len());
        for &frequency in frequencies {
            let value = self.complex(frequency)?;
            amplitudes.push(magnitude(value)?);
            let phase = super::lf_atan::argument(value[1], value[0], alternate);
            if !phase.is_finite() {
                return Err(invalid("LF analytic phase must remain finite"));
            }
            phases.push(phase);
        }
        let reference = magnitude(self.complex(200.0)?)?;
        if reference <= 0.0 {
            return Err(invalid("LF analytic reference magnitude must be positive"));
        }
        for amplitude in &mut amplitudes {
            *amplitude /= reference;
            if !amplitude.is_finite() {
                return Err(invalid(
                    "LF analytic normalized amplitude must remain finite",
                ));
            }
        }
        Ok((amplitudes, phases))
    }
}

pub(super) fn magnitude([real, imaginary]: [f32; 2]) -> Result<f32> {
    complex::finite([real, imaginary])?;
    let imaginary = f64::from(imaginary);
    let real = f64::from(real);
    let imaginary_square = imaginary * imaginary;
    let real_square = real * real;
    let result = (imaginary_square + real_square).sqrt() as f32;
    if result.is_finite() {
        Ok(result)
    } else {
        Err(invalid("LF analytic magnitude must remain finite"))
    }
}
