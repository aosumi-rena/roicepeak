use super::super::{
    Budget,
    matrix_storage::{Matrix, Workspace},
};
use crate::{
    Result, invalid,
    tensor::{Tensor, checked_elements},
};

#[derive(Debug)]
pub(super) struct Transpose {
    taps: Vec<Matrix>,
}

impl Transpose {
    pub(super) fn new(taps: Vec<Matrix>) -> Result<Self> {
        let first = taps
            .first()
            .ok_or_else(|| invalid("transpose convolution requires taps"))?;
        let (inputs, outputs) = (first.cols(), first.rows());
        if inputs == 0
            || outputs == 0
            || taps
                .iter()
                .any(|tap| tap.rows() != outputs || tap.cols() != inputs)
        {
            return Err(invalid("transpose convolution tap dimensions differ"));
        }
        Ok(Self {
            taps: taps
                .into_iter()
                .map(Matrix::into_product_kernel)
                .collect::<Result<Vec<_>>>()?,
        })
    }

    pub(super) fn channels(&self) -> (usize, usize) {
        (self.taps[0].cols(), self.taps[0].rows())
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        crate::memory::nested(&self.taps, Matrix::estimated_bytes)
    }

    pub(super) fn run(
        &self,
        input: &Tensor,
        step: usize,
        dilation: usize,
        bias: Option<&[f32]>,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let (inputs, outputs) = self.channels();
        if input.cols() != inputs
            || step == 0
            || dilation == 0
            || bias.is_some_and(|values| values.len() != outputs)
        {
            return Err(invalid("transpose convolution input dimensions differ"));
        }
        let span = (self.taps.len() - 1)
            .checked_mul(dilation)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| invalid("transpose convolution span overflow"))?;
        let frames = if input.rows() == 0 {
            0
        } else {
            (input.rows() - 1)
                .checked_mul(step)
                .and_then(|value| value.checked_add(span))
                .ok_or_else(|| invalid("transpose convolution frame overflow"))?
        };
        let work_per_input = self.taps.iter().try_fold(0usize, |sum, tap| {
            sum.checked_add(tap.elements())
                .ok_or_else(|| invalid("transpose convolution arithmetic overflow"))
        })?;
        let work = work_per_input
            .checked_mul(input.rows())
            .ok_or_else(|| invalid("transpose convolution arithmetic overflow"))?;
        let scratch = if frames == 0 {
            0
        } else {
            self.taps
                .iter()
                .map(Matrix::workspace_elements)
                .max()
                .unwrap_or(0)
        };
        let allocated = checked_elements(frames, outputs)?
            .checked_add(scratch)
            .ok_or_else(|| invalid("transpose convolution workspace overflow"))?;
        budget.spend(work, allocated)?;
        let mut output = Tensor::zeros(frames, outputs)?;
        let mut workspace = Workspace::default();
        for time in 0..input.rows() {
            for (tap, weight) in self.taps.iter().enumerate() {
                let at = time
                    .checked_mul(step)
                    .and_then(|value| {
                        tap.checked_mul(dilation)
                            .and_then(|offset| value.checked_add(offset))
                    })
                    .ok_or_else(|| invalid("transpose convolution index overflow"))?;
                weight.product_into(
                    input.row(time),
                    output.row_mut(at),
                    Matrix::add_mode(),
                    &mut workspace,
                )?;
            }
        }
        if let Some(bias) = bias {
            for row in output.values_mut().chunks_exact_mut(outputs) {
                for (value, bias) in row.iter_mut().zip(bias) {
                    *value = *bias + *value;
                }
            }
        }
        Ok(output)
    }
}
