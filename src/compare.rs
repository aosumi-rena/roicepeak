use crate::{Result, invalid, tensor::Tensor};

#[derive(Debug)]
pub struct Comparison {
    pub elements: usize,
    pub differing_words: usize,
    pub failures: usize,
    pub max_absolute: f64,
    pub max_relative: f64,
    pub rms: f64,
    pub max_ulp: u32,
    pub worst_index: usize,
}

pub fn compare(a: &Tensor, b: &Tensor, absolute: f64, relative: f64) -> Result<Comparison> {
    if !absolute.is_finite() || !relative.is_finite() || absolute < 0.0 || relative < 0.0 {
        return Err(invalid("invalid comparison tolerance"));
    }
    if a.rows() != b.rows() || a.cols() != b.cols() {
        return Err(invalid("comparison shape mismatch"));
    }
    let mut c = Comparison {
        elements: a.values().len(),
        differing_words: 0,
        failures: 0,
        max_absolute: 0.0,
        max_relative: 0.0,
        rms: 0.0,
        max_ulp: 0,
        worst_index: 0,
    };
    let mut sum = 0.0;
    for (i, (&reference, &actual)) in a.values().iter().zip(b.values()).enumerate() {
        let delta = (actual as f64 - reference as f64).abs();
        let scale = (reference as f64).abs();
        // Floor defines a finite descriptive relative metric near zero. The
        // admission gate below always uses the unmodified reference magnitude.
        let rel = delta / scale.max(1e-30);
        if reference.to_bits() != actual.to_bits() {
            c.differing_words += 1;
        }
        if delta > absolute + relative * scale {
            c.failures += 1;
        }
        if delta > c.max_absolute {
            c.max_absolute = delta;
            c.worst_index = i;
        }
        c.max_relative = c.max_relative.max(rel);
        c.max_ulp = c.max_ulp.max(ordered(reference).abs_diff(ordered(actual)));
        sum += delta * delta;
    }
    if c.elements != 0 {
        c.rms = (sum / c.elements as f64).sqrt();
    }
    Ok(c)
}
fn ordered(x: f32) -> u32 {
    let w = x.to_bits();
    if w & 0x80000000 != 0 {
        !w
    } else {
        w | 0x80000000
    }
}
