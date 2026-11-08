use super::{BANDS, HOP};
use crate::{Result, graph::Budget, invalid, tensor::Tensor};

const FACTOR: usize = HOP / BANDS;

pub(in crate::vocoder) struct Upsampler {
    previous: Option<Vec<f32>>,
    columns: Option<usize>,
    ended: bool,
}

impl Upsampler {
    pub(in crate::vocoder) fn new() -> Self {
        Self {
            previous: None,
            columns: None,
            ended: false,
        }
    }

    pub(in crate::vocoder) fn feed(
        &mut self,
        input: &Tensor,
        end: bool,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if self.columns.is_some_and(|columns| columns != input.cols()) {
            return Err(invalid("upsample stream channel count changed"));
        }
        if self.ended {
            if input.rows() != 0 {
                return Err(invalid("upsample input arrived after its end"));
            }
            return Tensor::zeros(0, input.cols());
        }
        let first = self.previous.is_none() && input.rows() != 0;
        let pairs = input.rows().saturating_sub(usize::from(first));
        let tail = end && (self.previous.is_some() || input.rows() != 0);
        let rows = pairs
            .checked_mul(FACTOR)
            .and_then(|rows| rows.checked_add(if first { FACTOR / 2 } else { 0 }))
            .and_then(|rows| rows.checked_add(if tail { FACTOR / 2 } else { 0 }))
            .ok_or_else(|| invalid("upsample overflow"))?;
        budget.spend(
            rows.saturating_mul(input.cols()).saturating_mul(2),
            rows.saturating_mul(input.cols()),
        )?;
        let mut output = Tensor::zeros(rows, input.cols())?;
        let mut cursor = 0;
        if first {
            for _ in 0..FACTOR / 2 {
                output.row_mut(cursor).copy_from_slice(input.row(0));
                cursor += 1;
            }
        }
        for index in usize::from(first)..input.rows() {
            let previous = if index == 0 {
                self.previous
                    .as_deref()
                    .ok_or_else(|| invalid("upsample previous row"))?
            } else {
                input.row(index - 1)
            };
            let current = input.row(index);
            for i in 0..FACTOR {
                let alpha = ((i as f64 + 0.5) / FACTOR as f64) as f32;
                for c in 0..input.cols() {
                    output.row_mut(cursor)[c] = (current[c] - previous[c]) * alpha + previous[c];
                }
                cursor += 1;
            }
        }
        if tail {
            let last = if input.rows() != 0 {
                input.row(input.rows() - 1)
            } else {
                self.previous
                    .as_deref()
                    .ok_or_else(|| invalid("upsample last row"))?
            };
            for _ in 0..FACTOR / 2 {
                output.row_mut(cursor).copy_from_slice(last);
                cursor += 1;
            }
        }
        if input.rows() != 0 {
            let previous = self.previous.get_or_insert_with(Vec::new);
            previous.clear();
            previous.extend_from_slice(input.row(input.rows() - 1));
        }
        self.columns = Some(input.cols());
        self.ended = end;
        Ok(output)
    }
}
