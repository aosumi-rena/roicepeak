use super::super::{Budget, matrix_storage::Matrix, parallel::FallibleRows};
use crate::{
    Result, invalid,
    tensor::{Tensor, checked_elements},
};

#[path = "row.rs"]
mod ordered;

#[derive(Debug)]
pub(super) struct ForwardFloat {
    outputs: usize,
    inputs: usize,
    kernel: usize,
    stride: usize,
    coefficients: Vec<f32>,
}

impl ForwardFloat {
    pub(super) fn new(taps: Vec<Matrix>) -> Result<Self> {
        #[cfg(target_arch = "x86_64")]
        let available = std::arch::is_x86_feature_detected!("avx")
            && std::arch::is_x86_feature_detected!("fma");
        #[cfg(not(target_arch = "x86_64"))]
        let available = false;
        if !available {
            return Err(invalid("float convolution CPU features are unavailable"));
        }
        let Some(Matrix::Dense(first)) = taps.first() else {
            return Err(invalid("float convolution requires dense taps"));
        };
        let (outputs, inputs, kernel) = (first.rows(), first.cols(), taps.len());
        if outputs == 0 || inputs == 0 {
            return Err(invalid("float convolution requires positive dimensions"));
        }
        let stride = outputs
            .div_ceil(8)
            .checked_mul(8)
            .ok_or_else(|| invalid("float convolution channel overflow"))?;
        let input_rows = checked_elements(kernel, inputs)?;
        let count = checked_elements(input_rows, stride)?;
        let mut coefficients = vec![0.0; count];
        for (tap, matrix) in taps.into_iter().enumerate() {
            let Matrix::Dense(weight) = matrix else {
                return Err(invalid("float convolution requires dense taps"));
            };
            if weight.rows() != outputs || weight.cols() != inputs {
                return Err(invalid("float convolution tap dimensions differ"));
            }
            for output in 0..outputs {
                for (input, &value) in weight.row(output).iter().enumerate() {
                    coefficients[(tap * inputs + input) * stride + output] = value;
                }
            }
        }
        Ok(Self {
            outputs,
            inputs,
            kernel,
            stride,
            coefficients,
        })
    }

    pub(super) fn channels(&self) -> (usize, usize) {
        (self.inputs, self.outputs)
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        crate::memory::values(&self.coefficients)
    }

    pub(super) fn run(
        &self,
        input: &Tensor,
        step: usize,
        padding: usize,
        dilation: usize,
        bias: Option<&[f32]>,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if input.cols() != self.inputs
            || step == 0
            || dilation == 0
            || bias.is_some_and(|values| values.len() != self.outputs)
        {
            return Err(invalid("float convolution input dimensions differ"));
        }
        let span = (self.kernel - 1)
            .checked_mul(dilation)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(|| invalid("convolution span overflow"))?;
        let padded = padding
            .checked_mul(2)
            .and_then(|n| input.rows().checked_add(n))
            .ok_or_else(|| invalid("convolution padding overflow"))?;
        let frames = if padded < span {
            0
        } else {
            (padded - span) / step + 1
        };
        let output_elements = checked_elements(frames, self.outputs)?;
        let macs_per_row = self.coefficients.len();
        let macs = frames
            .checked_mul(macs_per_row)
            .ok_or_else(|| invalid("float convolution arithmetic overflow"))?;
        let plan = FallibleRows::new(frames, macs_per_row);
        let workspace_elements = checked_elements(self.kernel, self.inputs)?;
        let all_workspaces = checked_elements(plan.chunks(), workspace_elements)?;
        let elements = output_elements
            .checked_add(all_workspaces)
            .ok_or_else(|| invalid("float convolution allocation overflow"))?;
        budget.spend(macs, elements)?;
        let mut output = Tensor::zeros(frames, self.outputs)?;
        if frames == 0 {
            return Ok(output);
        }
        plan.execute(output.values_mut(), self.outputs, |first, target| {
            let mut queued = vec![0.0; workspace_elements];
            for (offset, row) in target.chunks_exact_mut(self.outputs).enumerate() {
                let time = first + offset;
                for tap in 0..self.kernel {
                    let at = time
                        .checked_mul(step)
                        .and_then(|n| tap.checked_mul(dilation).and_then(|k| n.checked_add(k)))
                        .ok_or_else(|| invalid("float convolution input index overflow"))?;
                    let slice = &mut queued[tap * self.inputs..(tap + 1) * self.inputs];
                    if let Some(source) = at.checked_sub(padding).filter(|n| *n < input.rows()) {
                        slice.copy_from_slice(input.row(source));
                    } else {
                        slice.fill(0.0);
                    }
                }
                if !ordered::row(
                    &self.coefficients,
                    self.stride,
                    self.inputs,
                    self.kernel,
                    &queued,
                    self.inputs,
                    true,
                    row,
                    None,
                ) {
                    return Err(invalid(
                        "float convolution product dimensions or CPU features are unavailable",
                    ));
                }
                if let Some(bias) = bias {
                    for (value, bias) in row.iter_mut().zip(bias) {
                        *value = *bias + *value;
                    }
                }
            }
            Ok(())
        })?;
        Ok(output)
    }
}
