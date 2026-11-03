use super::Accumulate;
use crate::{Result, invalid, tensor::Tensor};

#[allow(unsafe_code)]
#[path = "fused_product.rs"]
mod product;

#[derive(Debug)]
pub struct FusedMatrix {
    outputs: usize,
    inputs: usize,
    stride: usize,
    coefficients: Vec<f32>,
}

impl FusedMatrix {
    pub fn new(weight: Tensor) -> Result<Self> {
        let outputs = weight.rows();
        let inputs = weight.cols();
        if outputs == 0 || inputs == 0 {
            return Err(invalid("float matrix requires positive dimensions"));
        }
        let stride = outputs.div_ceil(8) * 8;
        let count = crate::tensor::checked_elements(inputs, stride)?;
        let mut coefficients = vec![0.0; count];
        for output in 0..outputs {
            for (input, &value) in weight.row(output).iter().enumerate() {
                coefficients[input * stride + output] = value;
            }
        }
        Ok(Self {
            outputs,
            inputs,
            stride,
            coefficients,
        })
    }

    pub fn rows(&self) -> usize {
        self.outputs
    }

    pub fn cols(&self) -> usize {
        self.inputs
    }

    pub fn work_elements(&self) -> usize {
        self.coefficients.len()
    }

    pub fn estimated_bytes(&self) -> usize {
        crate::memory::values(&self.coefficients)
    }

    pub fn product_into(&self, input: &[f32], output: &mut [f32], mode: Accumulate) -> Result<()> {
        if input.len() != self.inputs || output.len() != self.outputs {
            return Err(invalid("float matrix product dimensions differ"));
        }
        if !product::product_into(
            &self.coefficients,
            self.stride,
            input,
            output,
            matches!(mode, Accumulate::Add { .. }),
        ) {
            return Err(invalid("float matrix product CPU features are unavailable"));
        }
        Ok(())
    }
}
