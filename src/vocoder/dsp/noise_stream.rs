use super::BANDS;
use crate::{
    Result,
    graph::Budget,
    invalid,
    tensor::{MAX_ELEMENTS, Tensor},
};

pub(in crate::vocoder) struct Noise {
    state: u64,
    pending: Option<f32>,
    fused: bool,
}

impl Noise {
    pub(in crate::vocoder) fn new(fused: bool) -> Self {
        Self {
            state: 1,
            pending: None,
            fused,
        }
    }

    pub(in crate::vocoder) fn generate(
        &mut self,
        rows: usize,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        budget.spend(rows.saturating_mul(BANDS * 64), rows.saturating_mul(BANDS))?;
        let count = rows
            .checked_mul(BANDS)
            .filter(|n| *n <= MAX_ELEMENTS)
            .ok_or_else(|| invalid("postfilter noise sample budget"))?;
        let mut values = Vec::with_capacity(count);
        while values.len() < count {
            values.push(self.normal());
        }
        Tensor::new(rows, BANDS, values)
    }

    fn normal(&mut self) -> f32 {
        if let Some(value) = self.pending.take() {
            return value;
        }
        loop {
            self.state = self.state * 48271 % 2147483647;
            let u = (self.state as f32 - 1.0) * (2.0f32).powi(-31);
            let mut x = u + u - 1.0;
            self.state = self.state * 48271 % 2147483647;
            let u = (self.state as f32 - 1.0) * (2.0f32).powi(-31);
            let mut y = u + u - 1.0;
            let mut radius = x * x + y * y;
            if x == 0.0 || y == 0.0 || radius >= 1.0 {
                continue;
            }
            let log_radius = if radius > 0.0001 {
                super::super::postfilter_log::logarithm(radius, self.fused)
            } else {
                let exponent = ((x.abs().max(y.abs()).to_bits() >> 23) & 0xff) as i32 - 127;
                let factor = 2.0f32.powi(-exponent);
                x *= factor;
                y *= factor;
                radius = x * x + y * y;
                (exponent as f32 * f32::from_bits(0x3fb1_7218))
                    + super::super::postfilter_log::logarithm(radius, self.fused)
            };
            let scale = (log_radius * -2.0 / radius).sqrt();
            let first = x * scale + 0.0;
            let second = y * scale + 0.0;
            self.pending = Some(second);
            return first;
        }
    }
}
