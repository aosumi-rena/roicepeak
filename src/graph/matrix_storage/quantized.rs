#[cfg(target_arch = "x86_64")]
#[path = "integer.rs"]
mod integer;

use crate::{Result, dnni::Node, invalid, tensor::u32_at};

#[derive(Debug)]
pub struct QuantizedMatrix {
    outputs: usize,
    inputs: usize,
    input_scale: f32,
    row_factors: Vec<f32>,
    coefficients: Vec<i16>,
}

#[derive(Clone, Copy)]
pub enum Accumulate {
    Replace,
    Add { fused: bool },
}

#[derive(Default)]
pub struct Workspace {
    pub(super) input: Vec<i16>,
    pub(super) output: Vec<i16>,
}

impl QuantizedMatrix {
    pub fn from_node(node: &Node<'_>) -> Result<Self> {
        if node.tag != "prim4" || !node.children.is_empty() {
            return Err(invalid("quantized matrix primitive"));
        }
        let p = node.payload;
        if u32_at(p, 0)? != 16 || u32_at(p, 8)? != 0 {
            return Err(invalid("unsupported quantized matrix descriptor"));
        }
        let field = u32_at(p, 4)? as i32;
        if !(-15..=30).contains(&field) {
            return Err(invalid("unsupported quantized matrix scale domain"));
        }
        // These power-of-two scale factors are exact in the accepted range.
        let input_scale = f32::from_bits(((127 + field) as u32) << 23);
        let inverse_scale = f32::from_bits(((127 - field) as u32) << 23);
        let outputs = u32_at(p, 12)? as usize;
        if outputs == 0 || outputs > crate::tensor::MAX_ELEMENTS {
            return Err(invalid("quantized output dimension"));
        }
        let header = 16 + outputs * 4;
        if u32_at(p, header)? as usize != outputs {
            return Err(invalid("quantized matrix scale count"));
        }
        let inputs = u32_at(p, header + 4)? as usize;
        let count = crate::tensor::checked_elements(outputs, inputs)?;
        if p.len() != header + 8 + count * 2 {
            return Err(invalid("quantized matrix payload length"));
        }
        let row_factors = p[16..header]
            .chunks_exact(4)
            .map(|word| f32::from_le_bytes(word.try_into().unwrap()) * inverse_scale)
            .collect::<Vec<_>>();
        if row_factors.iter().any(|value| !value.is_finite()) {
            return Err(invalid("quantized matrix row scale"));
        }
        let coefficients = p[header + 8..]
            .chunks_exact(2)
            .map(|word| i16::from_le_bytes(word.try_into().unwrap()))
            .collect();
        Ok(Self {
            outputs,
            inputs,
            input_scale,
            row_factors,
            coefficients,
        })
    }

    pub fn rows(&self) -> usize {
        self.outputs
    }

    pub fn cols(&self) -> usize {
        self.inputs
    }

    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            crate::memory::values(&self.row_factors),
            crate::memory::values(&self.coefficients),
        ])
    }

    pub fn product_into(
        &self,
        input: &[f32],
        output: &mut [f32],
        accumulate: Accumulate,
        workspace: &mut Workspace,
    ) -> Result<()> {
        if input.len() != self.inputs || output.len() != self.outputs {
            return Err(invalid("quantized matrix product dimensions"));
        }
        if input.iter().any(|value| !value.is_finite())
            || matches!(accumulate, Accumulate::Add { .. })
                && output.iter().any(|value| !value.is_finite())
        {
            return Err(invalid("nonfinite quantized matrix operand"));
        }
        workspace.input.resize(self.inputs, 0);
        workspace.output.resize(self.outputs, 0);
        super::input_quantization::quantize_into(input, self.input_scale, &mut workspace.input);
        integer_product(
            &self.coefficients,
            &workspace.input,
            &mut workspace.output,
            false,
        )?;
        convert_product(&workspace.output, &self.row_factors, output, accumulate);
        if output.iter().any(|value| !value.is_finite()) {
            return Err(invalid("nonfinite quantized matrix result"));
        }
        Ok(())
    }
}

impl Workspace {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            crate::memory::values(&self.input),
            crate::memory::values(&self.output),
        ])
    }
}

fn integer_product(weights: &[i16], input: &[i16], output: &mut [i16], add: bool) -> Result<()> {
    // from_node and product_into already validate the complete I*O layout.
    #[cfg(target_arch = "x86_64")]
    {
        integer::integer_product(weights, input, output, add);
        Ok(())
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (weights, input, output, add);
        Err(invalid(
            "quantized integer backend is unavailable on this architecture",
        ))
    }
}

fn convert_product(input: &[i16], factors: &[f32], output: &mut [f32], accumulate: Accumulate) {
    for ((target, &value), &factor) in output.iter_mut().zip(input).zip(factors) {
        let value = f32::from(value);
        *target = match accumulate {
            Accumulate::Replace => value * factor,
            Accumulate::Add { fused: true } => value.mul_add(factor, *target),
            Accumulate::Add { fused: false } => value * factor + *target,
        };
    }
}
