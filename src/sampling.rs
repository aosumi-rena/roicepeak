//! Explicit single-pass acoustic sampler and reproducible random inputs.
use crate::{
    Result,
    dnni::Node,
    graph::{Budget, Graph},
    invalid,
    sequence::concatenate,
    tensor::{Tensor, u32_at},
};

#[derive(Debug)]
pub struct Sampler {
    pub channels: usize,
    embedding_channels: usize,
    embedding_scale: f32,
    input: Graph,
    conditional: Graph,
    output: Graph,
}
impl Sampler {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.input.estimated_bytes(),
            self.conditional.estimated_bytes(),
            self.output.estimated_bytes(),
        ])
    }

    pub fn compile(n: &Node<'_>) -> Result<Self> {
        if n.tag != "_didsv0" || n.children.len() != 3 || n.payload.len() != 12 {
            return Err(invalid("single-pass sampler structure"));
        }
        let channels = u32_at(n.payload, 0)? as usize;
        let embedding_channels = u32_at(n.payload, 4)? as usize;
        let embedding_scale = f32::from_bits(u32_at(n.payload, 8)?);
        if channels == 0
            || embedding_channels == 0
            || embedding_channels % 2 != 0
            || !embedding_scale.is_finite()
        {
            return Err(invalid("sampler dimensions or scale"));
        }
        Ok(Self {
            channels,
            embedding_channels,
            embedding_scale,
            input: Graph::compile(&n.children[0])?,
            conditional: Graph::compile(&n.children[1])?,
            output: Graph::compile(&n.children[2])?,
        })
    }
    pub fn embed(&self, values: &[f32], budget: &mut Budget) -> Result<Tensor> {
        budget.spend(
            values
                .len()
                .saturating_mul(self.embedding_channels)
                .saturating_mul(64),
            values.len().saturating_mul(self.embedding_channels),
        )?;
        let mut out = Tensor::zeros(values.len(), self.embedding_channels)?;
        let half = self.embedding_channels / 2;
        for (t, v) in values.iter().enumerate() {
            for i in 0..half {
                let exponent = (i as f32 * 2.0) / self.embedding_channels as f32;
                let angle = (v * self.embedding_scale) / 10000.0f32.powf(exponent);
                out.row_mut(t)[i] = angle.sin();
                out.row_mut(t)[i + half] = angle.cos();
            }
        }
        out.ensure_finite()?;
        Ok(out)
    }
    pub fn run(
        &self,
        features: &Tensor,
        context: &Tensor,
        noise: &Tensor,
        shape: f32,
        offset: f32,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if features.rows() != context.rows()
            || noise.rows() != context.rows()
            || noise.cols() != self.channels
        {
            return Err(invalid("sampler input dimensions"));
        }
        let shape = self.embed(&vec![shape; context.rows()], budget)?;
        let offset = self.embed(&vec![offset; context.rows()], budget)?;
        let condition = concatenate(&[context, &shape, &offset, noise], budget)?;
        let x = self.input.run_with(features, budget)?;
        let y = self.conditional.run_conditioned(&x, &condition, budget)?;
        self.output.run_with(&y, budget)
    }
}

/// MT19937 plus float32 Marsaglia polar. Reseeding preserves the spare normal.
pub struct NormalStream {
    state: [u32; 624],
    index: usize,
    spare: Option<f32>,
}
impl Default for NormalStream {
    fn default() -> Self {
        Self::new(5489)
    }
}
impl NormalStream {
    pub fn new(seed: u32) -> Self {
        let mut s = Self {
            state: [0; 624],
            index: 624,
            spare: None,
        };
        s.reseed(seed);
        s
    }
    pub fn reseed(&mut self, seed: u32) {
        self.state[0] = seed;
        for i in 1..624 {
            let p = self.state[i - 1];
            self.state[i] = 1812433253u32
                .wrapping_mul(p ^ (p >> 30))
                .wrapping_add(i as u32);
        }
        self.index = 624;
    }
    pub fn word(&mut self) -> u32 {
        if self.index == 624 {
            for i in 0..624 {
                let y = (self.state[i] & 0x80000000) | (self.state[(i + 1) % 624] & 0x7fffffff);
                self.state[i] = self.state[(i + 397) % 624]
                    ^ (y >> 1)
                    ^ if y & 1 != 0 { 0x9908b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c5680;
        y ^= (y << 15) & 0xefc60000;
        y ^= y >> 18;
        y
    }
    pub fn normal(&mut self) -> f32 {
        if let Some(value) = self.spare.take() {
            return value;
        }
        loop {
            let u = self.word() as f32 * (2.0f32).powi(-32);
            let x = u + u - 1.0;
            let u = self.word() as f32 * (2.0f32).powi(-32);
            let y = u + u - 1.0;
            let s = x * x + y * y;
            if s > 1.0 || s == 0.0 {
                continue;
            }
            let factor = (s.ln() * -2.0 / s).sqrt();
            self.spare = Some(y * factor);
            return x * factor;
        }
    }
}
fn hash(bytes: &[u8], mut state: u32) -> u32 {
    for b in bytes {
        state = (state ^ u32::from(*b)).wrapping_mul(0x01000193);
    }
    state
}
pub fn phoneme_seeds(phones: &[String], variations: &[i32], base: i32) -> Result<Vec<u32>> {
    if phones.len() != variations.len() {
        return Err(invalid("phone/variation counts differ"));
    }
    let start = hash(&base.to_le_bytes(), 0x811c9dc5);
    let mut seeds = Vec::with_capacity(phones.len());
    for i in 0..phones.len() {
        let mut h = start;
        for p in &phones[i.saturating_sub(2)..=i] {
            h = hash(p.as_bytes(), h);
        }
        seeds.push(hash(&variations[i].to_le_bytes(), h));
    }
    Ok(seeds)
}
pub fn frame_noise(
    counts: &[usize],
    channels: usize,
    scale: f32,
    seeds: Option<&[u32]>,
    stream: &mut NormalStream,
    budget: &mut Budget,
) -> Result<Tensor> {
    if !scale.is_finite() || seeds.is_some_and(|s| s.len() != counts.len()) {
        return Err(invalid("noise parameters"));
    }
    let rows = counts
        .iter()
        .try_fold(0usize, |n, c| n.checked_add(*c))
        .ok_or_else(|| invalid("noise frame overflow"))?;
    budget.spend(
        rows.saturating_mul(channels).saturating_mul(64),
        rows.saturating_mul(channels),
    )?;
    let mut out = Tensor::zeros(rows, channels)?;
    let mut row = 0;
    for (i, count) in counts.iter().enumerate() {
        if *count == 0 {
            continue;
        }
        if let Some(seeds) = seeds {
            stream.reseed(seeds[i]);
        }
        for _ in 0..*count {
            for v in out.row_mut(row) {
                *v = stream.normal() * scale;
            }
            row += 1;
        }
    }
    out.ensure_finite()?;
    Ok(out)
}
