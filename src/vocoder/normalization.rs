//! Mode-zero voicing and expanded pitch features for vocoder inputs.
use crate::{
    Result,
    dnni::Node,
    graph::Budget,
    invalid,
    normalization::Normalizer,
    tensor::{MAX_ELEMENTS, Tensor, u32_at},
};

pub(super) struct InputConfiguration {
    pub(super) force_voiced: bool,
    expansion: Option<CosineExpansion>,
}

struct CosineExpansion {
    bank_width: usize,
    output_width: usize,
}

impl InputConfiguration {
    pub(super) fn compile(node: &Node<'_>, normalizer: &Normalizer) -> Result<Self> {
        let Some(width) = node.payload.get(32..36) else {
            return Ok(Self {
                force_voiced: false,
                expansion: None,
            });
        };
        let bank_width = u32::from_le_bytes(width.try_into().unwrap()) as usize;
        let version_two = node.tag == "_vocfv2" && node.payload.len() == 52;
        let force_voiced = version_two && u32_at(node.payload, 48)? != 0;
        if bank_width < 2 && !force_voiced {
            return Ok(Self {
                force_voiced: false,
                expansion: None,
            });
        }
        if (!(node.tag == "_vocfv1" && node.payload.len() == 48) && !version_two)
            || node.children.len() != 4
        {
            return Err(invalid("unsupported vocoder input configuration"));
        }
        for (offset, value) in [
            (4, 1),
            (16, 72),
            (20, 1),
            (24, 1),
            (28, 0),
            (36, 1),
            (40, 1),
            (44, 0),
        ] {
            if u32_at(node.payload, offset)? != value {
                return Err(invalid("unsupported vocoder input configuration"));
            }
        }
        let output_width = if bank_width < 2 {
            72
        } else {
            bank_width
                .checked_add(71)
                .filter(|width| *width <= MAX_ELEMENTS)
                .ok_or_else(|| invalid("vocoder cosine-bank width"))?
        };
        if u32_at(node.payload, 0)? as usize != output_width {
            return Err(invalid("vocoder declared normalized output width"));
        }
        let statistics_match = match normalizer {
            Normalizer::Range {
                minimum, maximum, ..
            } => minimum
                .as_ref()
                .zip(maximum.as_ref())
                .is_some_and(|(minimum, maximum)| minimum.len() == 72 && maximum.len() == 72),
            Normalizer::Standard {
                mean,
                deviation,
                reciprocal,
            } => {
                mean.len() == 72
                    && deviation.len() == 72
                    && reciprocal.as_ref().is_none_or(|values| values.len() == 72)
            }
        };
        if !statistics_match {
            return Err(invalid("vocoder input normalizer width"));
        }
        Ok(Self {
            force_voiced,
            expansion: (bank_width >= 2).then_some(CosineExpansion {
                bank_width,
                output_width,
            }),
        })
    }

    pub(super) fn expand(
        &self,
        normalized: Tensor,
        fused: bool,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        match &self.expansion {
            Some(expansion) => expansion.expand(&normalized, fused, budget),
            None => Ok(normalized),
        }
    }
}

impl CosineExpansion {
    fn expand(&self, normalized: &Tensor, fused: bool, budget: &mut Budget) -> Result<Tensor> {
        if normalized.cols() != 72 {
            return Err(invalid("vocoder cosine-bank input width"));
        }
        budget.spend(
            normalized
                .rows()
                .saturating_mul(self.bank_width)
                .saturating_mul(32),
            normalized.rows().saturating_mul(self.output_width),
        )?;
        let mut output = Tensor::zeros(normalized.rows(), self.output_width)?;
        for t in 0..normalized.rows() {
            let input = normalized.row(t);
            let row = output.row_mut(t);
            row[0] = input[0];
            let position = input[1] * 0.5 + 0.5;
            for k in 0..self.bank_width {
                let phase = (k as f32 - 1.0) / self.bank_width as f32;
                let angle = (position - phase) * std::f32::consts::TAU;
                let cosine = super::response_trig::cosine(angle, fused);
                let bank = (f64::from(cosine) * 0.5 + 0.5) as f32;
                row[1 + k] = bank * 2.0 + -1.0;
            }
            row[1 + self.bank_width..].copy_from_slice(&input[2..]);
        }
        output.ensure_finite()?;
        Ok(output)
    }
}
