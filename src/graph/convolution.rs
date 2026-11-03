use super::{Budget, matrix, payload, vector};
use crate::{
    Result,
    dnni::Node,
    invalid,
    tensor::{MAX_ELEMENTS, Tensor, u32_at},
};

#[derive(Debug)]
pub struct Convolution {
    taps: Vec<Tensor>,
    bias: Option<Vec<f32>>,
    stride: usize,
    padding: usize,
    dilation: usize,
    transpose: bool,
}
impl Convolution {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            crate::memory::nested(&self.taps, Tensor::estimated_bytes),
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
        (self.taps[0].cols(), self.taps[0].rows())
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
                taps.push(matrix(child)?);
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
        Ok(Self {
            taps,
            bias,
            stride,
            padding,
            dilation,
            transpose: n.tag == "modl2",
        })
    }
    pub(super) fn run(&self, x: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        let outputs = self.taps[0].rows();
        if x.cols() != self.taps[0].cols() {
            return Err(invalid("convolution input dimension"));
        }
        let span = (self.taps.len() - 1)
            .checked_mul(self.dilation)
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| invalid("convolution span overflow"))?;
        let full = if self.transpose {
            if x.rows() == 0 {
                0
            } else {
                (x.rows() - 1)
                    .checked_mul(self.stride)
                    .and_then(|v| v.checked_add(span))
                    .ok_or_else(|| invalid("convolution frame overflow"))?
            }
        } else {
            0
        };
        let frames = if self.transpose {
            full.saturating_sub(2 * self.padding)
        } else {
            let padded = x
                .rows()
                .checked_add(2 * self.padding)
                .ok_or_else(|| invalid("convolution padding overflow"))?;
            if padded < span {
                0
            } else {
                (padded - span) / self.stride + 1
            }
        };
        let macs = (if self.transpose { x.rows() } else { frames })
            .saturating_mul(outputs)
            .saturating_mul(x.cols())
            .saturating_mul(self.taps.len());
        budget.spend(macs, frames.saturating_mul(outputs))?;
        let mut y = Tensor::zeros(frames, outputs)?;
        if self.transpose {
            if let Some(b) = &self.bias {
                for t in 0..frames {
                    y.row_mut(t).copy_from_slice(b);
                }
            }
            for t in 0..x.rows() {
                for (k, tap) in self.taps.iter().enumerate() {
                    let at = t * self.stride + k * self.dilation;
                    if at < self.padding || at >= full.saturating_sub(self.padding) {
                        continue;
                    }
                    accumulate(tap, x.row(t), y.row_mut(at - self.padding));
                }
            }
        } else {
            let macs_per_row = outputs
                .saturating_mul(x.cols())
                .saturating_mul(self.taps.len());
            super::parallel::for_rows(y.values_mut(), outputs, macs_per_row, |first, target| {
                for (offset, row) in target.chunks_exact_mut(outputs).enumerate() {
                    let t = first + offset;
                    for (k, tap) in self.taps.iter().enumerate() {
                        let at = t * self.stride + k * self.dilation;
                        if at < self.padding || at - self.padding >= x.rows() {
                            continue;
                        }
                        let input = x.row(at - self.padding);
                        accumulate(tap, input, row);
                    }
                    if let Some(b) = &self.bias {
                        for (v, b) in row.iter_mut().zip(b) {
                            *v += b;
                        }
                    }
                }
            });
        }
        Ok(y)
    }
}

fn accumulate(tap: &Tensor, input: &[f32], output: &mut [f32]) {
    let grouped = output.len() / 4 * 4;
    for o in (0..grouped).step_by(4) {
        let sums = super::dot4(
            [tap.row(o), tap.row(o + 1), tap.row(o + 2), tap.row(o + 3)],
            input,
        );
        for lane in 0..4 {
            output[o + lane] += sums[lane];
        }
    }
    for o in grouped..output.len() {
        output[o] += super::dot(tap.row(o), input);
    }
}
