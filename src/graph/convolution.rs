use super::matrix_storage::{Accumulate, Matrix, Workspace};
use super::{Budget, payload, vector};
use crate::{
    Result,
    dnni::Node,
    invalid,
    tensor::{MAX_ELEMENTS, Tensor, u32_at},
};

#[path = "convolution/forward.rs"]
mod forward;
use forward::ForwardFloat;
#[path = "convolution/transpose.rs"]
mod transpose;
use transpose::Transpose;

#[derive(Debug)]
enum Weights {
    Packed(ForwardFloat),
    Transposed(Transpose),
    Matrices(Vec<Matrix>),
}

impl Weights {
    fn estimated_bytes(&self) -> usize {
        match self {
            Self::Packed(value) => value.estimated_bytes(),
            Self::Transposed(value) => value.estimated_bytes(),
            Self::Matrices(taps) => crate::memory::nested(taps, Matrix::estimated_bytes),
        }
    }
}

#[derive(Debug)]
pub struct Convolution {
    weights: Weights,
    bias: Option<Vec<f32>>,
    stride: usize,
    padding: usize,
    dilation: usize,
    transpose: bool,
}
impl Convolution {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.weights.estimated_bytes(),
            crate::memory::optional(&self.bias, crate::memory::values),
        ])
    }

    pub(super) fn operator_name(&self) -> &'static str {
        if self.transpose {
            "transpose_convolution"
        } else {
            "convolution"
        }
    }
    pub(crate) fn channels(&self) -> (usize, usize) {
        match &self.weights {
            Weights::Packed(value) => value.channels(),
            Weights::Transposed(value) => value.channels(),
            Weights::Matrices(taps) => (taps[0].cols(), taps[0].rows()),
        }
    }
    pub(super) fn from_node(n: &Node<'_>) -> Result<Self> {
        payload(n, 20)?;
        let cfg = (0..5)
            .map(|i| u32_at(n.payload, i * 4).map(|v| v as usize))
            .collect::<Result<Vec<_>>>()?;
        let [kernel, stride, padding, dilation, one] = cfg[..] else {
            unreachable!()
        };
        if kernel == 0
            || stride == 0
            || dilation == 0
            || one != 1
            || kernel > 4096
            || [stride, padding, dilation]
                .iter()
                .any(|v| *v > MAX_ELEMENTS)
        {
            return Err(invalid("invalid convolution configuration"));
        }
        let mut taps = Vec::new();
        let mut bias = None;
        for child in &n.children {
            if child.tag == "prim1" {
                if bias.is_some() {
                    return Err(invalid("multiple convolution biases"));
                }
                bias = Some(vector(child)?);
            } else {
                taps.push(Matrix::from_node(child)?);
            }
        }
        if taps.len() != kernel {
            return Err(invalid("convolution kernel/tap count differs"));
        }
        let (outputs, inputs) = (taps[0].rows(), taps[0].cols());
        if outputs == 0
            || taps
                .iter()
                .any(|t| t.rows() != outputs || t.cols() != inputs)
            || bias.as_ref().is_some_and(|b| b.len() != outputs)
        {
            return Err(invalid("convolution tensor dimensions differ"));
        }
        let transpose = n.tag == "modl2";
        let quantized = taps.iter().any(Matrix::is_quantized);
        if !transpose && quantized && taps.iter().any(|tap| !tap.is_quantized()) {
            return Err(invalid(
                "mixed dense and quantized convolution taps are unsupported",
            ));
        }
        if transpose && padding != 0 {
            return Err(invalid("transpose convolution padding must be zero"));
        }
        let weights = if transpose {
            Weights::Transposed(Transpose::new(taps)?)
        } else if quantized {
            Weights::Matrices(taps)
        } else {
            Weights::Packed(ForwardFloat::new(taps)?)
        };
        Ok(Self {
            weights,
            bias,
            stride,
            padding,
            dilation,
            transpose,
        })
    }
    pub(super) fn run(&self, x: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        if let Weights::Packed(value) = &self.weights {
            return value.run(
                x,
                self.stride,
                self.padding,
                self.dilation,
                self.bias.as_deref(),
                budget,
            );
        }
        if let Weights::Transposed(value) = &self.weights {
            return value.run(x, self.stride, self.dilation, self.bias.as_deref(), budget);
        }
        let Weights::Matrices(taps) = &self.weights else {
            unreachable!("float convolution returned before matrix execution");
        };
        let outputs = taps[0].rows();
        if x.cols() != taps[0].cols() {
            return Err(invalid("convolution input dimension"));
        }
        let span = (taps.len() - 1)
            .checked_mul(self.dilation)
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| invalid("convolution span overflow"))?;
        let padded = x
            .rows()
            .checked_add(2 * self.padding)
            .ok_or_else(|| invalid("convolution padding overflow"))?;
        let frames = if padded < span {
            0
        } else {
            (padded - span) / self.stride + 1
        };
        let macs = frames
            .saturating_mul(outputs)
            .saturating_mul(x.cols())
            .saturating_mul(taps.len());
        self.run_quantized(taps, x, frames, macs, budget)
    }
    fn run_quantized(
        &self,
        taps: &[Matrix],
        input: &Tensor,
        frames: usize,
        macs: usize,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let outputs = taps[0].rows();
        let macs_per_row = outputs
            .saturating_mul(input.cols())
            .saturating_mul(taps.len());
        let plan = super::parallel::FallibleRows::new(frames, macs_per_row);
        let scratch = if frames == 0 {
            0
        } else {
            taps[0]
                .workspace_elements()
                .saturating_mul(plan.chunks())
                .saturating_add(input.cols())
        };
        budget.spend(macs, frames.saturating_mul(outputs).saturating_add(scratch))?;
        let mut output = Tensor::zeros(frames, outputs)?;
        if frames == 0 {
            return Ok(output);
        }
        let zero = vec![0.0; input.cols()];
        plan.execute(output.values_mut(), outputs, |first, target| {
            let mut workspace = Workspace::default();
            for (offset, row) in target.chunks_exact_mut(outputs).enumerate() {
                let time = first + offset;
                for (index, tap) in taps.iter().enumerate() {
                    let at = time * self.stride + index * self.dilation;
                    let source = if at < self.padding || at - self.padding >= input.rows() {
                        zero.as_slice()
                    } else {
                        input.row(at - self.padding)
                    };
                    let mode = if index == 0 {
                        Accumulate::Replace
                    } else {
                        Matrix::add_mode()
                    };
                    tap.quantized_product_into(source, row, mode, &mut workspace)?;
                }
                if let Some(bias) = &self.bias {
                    for (value, bias) in row.iter_mut().zip(bias) {
                        *value += bias;
                    }
                }
            }
            Ok(())
        })?;
        Ok(output)
    }
}
