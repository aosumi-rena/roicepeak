use crate::{Result, invalid};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effects {
    periodic: Vec<[f32; 2]>,
    aperiodic: Vec<[f32; 2]>,
}

#[derive(Clone, Copy, Debug)]
pub enum SpectrumKind {
    Periodic,
    Aperiodic,
}

impl Effects {
    pub fn from_le_bytes(periodic: &[u8], aperiodic: &[u8]) -> Result<Self> {
        let decode = |bytes: &[u8]| -> Result<Vec<[f32; 2]>> {
            if bytes.len() % 8 != 0 || bytes.len() > 4096 * 8 {
                return Err(invalid(
                    "vocoder equalizer requires float32 coordinate/gain pairs",
                ));
            }
            Ok(bytes
                .chunks_exact(8)
                .map(|pair| {
                    [
                        f32::from_le_bytes(pair[..4].try_into().unwrap()),
                        f32::from_le_bytes(pair[4..].try_into().unwrap()),
                    ]
                })
                .collect())
        };
        Self::new(&decode(periodic)?, &decode(aperiodic)?)
    }

    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            crate::memory::values(&self.periodic),
            crate::memory::values(&self.aperiodic),
        ])
    }

    pub fn new(periodic: &[[f32; 2]], aperiodic: &[[f32; 2]]) -> Result<Self> {
        let prepare = |points: &[[f32; 2]]| -> Result<Vec<[f32; 2]>> {
            if points.len() > 4096 || points.iter().flatten().any(|value| !value.is_finite()) {
                return Err(invalid(
                    "vocoder equalizer points must be bounded and finite",
                ));
            }
            Ok(points
                .iter()
                .map(|[coordinate, gain]| [coordinate * 0.5, *gain])
                .collect())
        };
        Ok(Self {
            periodic: prepare(periodic)?,
            aperiodic: prepare(aperiodic)?,
        })
    }

    pub fn active(&self, tension: f32) -> bool {
        tension != 0.0 || !self.periodic.is_empty() || !self.aperiodic.is_empty()
    }

    pub fn apply(
        &self,
        packed: &mut [f32],
        kind: SpectrumKind,
        tension: f32,
        sample_rate: f32,
    ) -> Result<()> {
        if !self.active(tension) {
            return Ok(());
        }
        if packed.len() < 2
            || packed.len() % 2 != 0
            || packed.len() > 65_536
            || packed.iter().any(|value| !value.is_finite())
            || !tension.is_finite()
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
        {
            return Err(invalid("vocoder spectrum/effect dimensions or values"));
        }
        let bins = packed.len() / 2;
        let mut magnitudes = vec![0.0; bins];
        let mut phases = vec![0.0; bins];
        magnitudes[0] = fast_log(packed[0].abs());
        for index in 1..bins {
            let real = packed[index * 2];
            let imaginary = packed[index * 2 + 1];
            magnitudes[index] = fast_log(real * real + imaginary * imaginary) * 0.5;
            phases[index] = f64::from(imaginary).atan2(f64::from(real)) as f32;
        }
        let points: &[[f32; 2]] = match kind {
            SpectrumKind::Periodic => &self.periodic,
            SpectrumKind::Aperiodic if !self.periodic.is_empty() => &self.aperiodic,
            SpectrumKind::Aperiodic => &[][..],
        };
        equalize(&mut magnitudes, points, sample_rate * 0.5);
        if matches!(kind, SpectrumKind::Aperiodic) {
            let adjustment = (1.0f64 + 1e-8).ln();
            for magnitude in &mut magnitudes {
                *magnitude = (f64::from(*magnitude) + adjustment) as f32;
            }
        }
        packed[0] = fast_exp(magnitudes[0]);
        // The packed Nyquist scalar at index 1 is retained by this callback.
        for index in 1..bins {
            let amplitude = fast_exp(magnitudes[index]);
            packed[index * 2] = fast_sin(phases[index] + std::f32::consts::FRAC_PI_2) * amplitude;
            packed[index * 2 + 1] = fast_sin(phases[index]) * amplitude;
        }
        if packed.iter().any(|value| !value.is_finite()) {
            return Err(invalid("vocoder effect produced a non-finite spectrum"));
        }
        Ok(())
    }
}

fn fast_log(value: f32) -> f32 {
    let bits = value.to_bits();
    let fraction = f32::from_bits((bits & 0x7fffff) | 0x3f000000);
    let exponent = bits as f32 * f32::from_bits(0x34000000) + f32::from_bits(0xc2f87377);
    let linear = f32::from_bits(0xbfbfbf75) * fraction + exponent;
    let reciprocal = f32::from_bits(0xbfdce9a3) / (fraction + f32::from_bits(0x3eb444f9));
    (reciprocal + linear) * f32::from_bits(0x3f317218)
}

fn fast_exp(value: f32) -> f32 {
    let exponent = value * f32::from_bits(0x3fb8aa3b);
    let clipped = exponent.max(-126.0);
    let fraction = clipped - clipped.trunc() + if exponent < 0.0 { 1.0 } else { 0.0 };
    let base = clipped + f32::from_bits(0x42f28c51);
    let reciprocal = f32::from_bits(0x41ddd2fe) / (f32::from_bits(0x409af5f8) - fraction);
    let approximation = fraction * f32::from_bits(0xbfbebc8d) + (reciprocal + base);
    let bits = approximation * 8_388_608.0;
    f32::from_bits((bits as i64) as u32)
}

fn fast_sin(angle: f32) -> f32 {
    let periods = (angle * f32::from_bits(0x3e22f983)).trunc();
    let offset = if angle < 0.0 { -0.5 } else { 0.5 };
    let reduced = (periods + offset) * std::f32::consts::TAU - angle;
    let sign = reduced.to_bits() & 0x80000000;
    let curved = (reduced * f32::from_bits(0xbecf817b)) * reduced.abs();
    let parabola = reduced * f32::from_bits(0x3fa2f983) + curved;
    let square = parabola * parabola;
    let c = f32::from_bits(sign ^ 0xbb533217);
    let a = f32::from_bits(sign | 0x3c77ce9a);
    let b = f32::from_bits(sign | 0x3e5086d7);
    let correction = ((c * square + a) * square + b) * square;
    parabola * f32::from_bits(0x3f48d161) + correction
}

fn bin_index(value: f32) -> i32 {
    if !value.is_finite() || !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

fn equalize(magnitudes: &mut [f32], points: &[[f32; 2]], nyquist: f32) {
    let count = magnitudes.len();
    for (index, &[coordinate, gain]) in points.iter().enumerate() {
        let start = bin_index(coordinate / nyquist * count as f32).max(0) as usize;
        let (end, next_gain) = points.get(index + 1).map_or((count, gain), |[next, gain]| {
            (
                (bin_index(next / nyquist * count as f32).max(0) as usize).min(count),
                *gain,
            )
        });
        if start >= end {
            continue;
        }
        for (bin, magnitude) in magnitudes.iter_mut().enumerate().take(end).skip(start) {
            let interpolation = (bin - start) as f32 / (end - start) as f32;
            let gain = interpolation * (next_gain - gain) + gain;
            *magnitude += (f64::from(gain) / 8.685889611636938) as f32;
        }
    }
}
