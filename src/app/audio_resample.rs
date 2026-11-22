use std::{collections::HashMap, ops::Range};

const TAPS: usize = 64;
const LEFT: i64 = 31;
const MAX_PHASES: usize = 4096;

pub(super) struct Sinc {
    ratio: f64,
    cutoff: f64,
    phases: Option<HashMap<u64, [f64; TAPS]>>,
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn coefficient(delta: f64, cutoff: f64) -> f64 {
    let window = 0.42
        + 0.5 * (std::f64::consts::PI * delta / 32.0).cos()
        + 0.08 * (std::f64::consts::TAU * delta / 32.0).cos();
    let z = std::f64::consts::PI * cutoff * delta;
    let sinc = if z.abs() < 1e-12 {
        cutoff
    } else {
        cutoff * z.sin() / z
    };
    sinc * window
}

impl Sinc {
    pub(super) fn new(source: u32, target: u32) -> Self {
        Self {
            ratio: f64::from(source) / f64::from(target),
            cutoff: (f64::from(target) / f64::from(source)).min(1.0) * 0.95,
            phases: (target / gcd(source, target) <= 256).then(HashMap::new),
        }
    }

    fn position(&self, index: usize) -> f64 {
        index as f64 * self.ratio
    }

    pub(super) fn source_span(&self, start: usize, count: usize, total: usize) -> Range<usize> {
        let first = self.position(start).floor() as i64 - LEFT;
        let end =
            self.position(start + count.saturating_sub(1)).floor() as i64 - LEFT + TAPS as i64;
        (first.max(0) as usize).min(total)..(end.max(0) as usize).min(total)
    }

    pub(super) fn window(
        &mut self,
        input: &[f32],
        origin: usize,
        start: usize,
        count: usize,
    ) -> Vec<f32> {
        (start..start + count)
            .map(|index| self.sample(input, origin, index))
            .collect()
    }

    fn sample(&mut self, input: &[f32], origin: usize, index: usize) -> f32 {
        let at = self.position(index);
        let center = at.floor() as i64;
        let first = center - LEFT;
        let origin = origin as i64;
        let start = first.max(origin);
        let end = (first + TAPS as i64).min(origin + input.len() as i64);
        if start >= end {
            return 0.0;
        }
        let phase = at - center as f64;
        let make = || {
            std::array::from_fn(|tap| coefficient(phase - (tap as i64 - LEFT) as f64, self.cutoff))
        };
        let uncached;
        let weights = if let Some(cache) = &mut self.phases {
            let key = phase.to_bits();
            if cache.len() == MAX_PHASES && !cache.contains_key(&key) {
                cache.clear();
            }
            cache.entry(key).or_insert_with(make)
        } else {
            uncached = make();
            &uncached
        };
        let mut value = 0.0;
        let mut weight = 0.0;
        for (&sample, &w) in input[(start - origin) as usize..(end - origin) as usize]
            .iter()
            .zip(&weights[(start - first) as usize..(end - first) as usize])
        {
            value += f64::from(sample) * w;
            weight += w;
        }
        if weight.abs() > 1e-12 {
            (value / weight) as f32
        } else {
            0.0
        }
    }
}
