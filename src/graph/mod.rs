//! A strict supported subset: unknown operators never become identity nodes.
mod attention;
mod conditional;
mod convolution;
mod filterbank;
mod generated_activation;
mod parallel;
pub mod profiling;
mod recurrent;

use crate::{
    Result,
    activation::Activation,
    dnni::Node,
    invalid,
    tensor::{Tensor, u32_at},
};
use attention::Attention;
use conditional::{Conditional, Gated};
use convolution::Convolution;
use filterbank::Filterbank;
use generated_activation::{GeneratedActivation, Kind as GeneratedActivationKind};
use recurrent::Gru;

#[derive(Debug)]
pub enum Graph {
    Sequential(Vec<Graph>),
    Affine(Affine),
    Activation(Activation),
    GeneratedActivation(GeneratedActivation),
    Convolution(Convolution),
    Gru(Gru),
    BidirectionalGru(Gru, Gru),
    ChannelAffine(Vec<f32>, Vec<f32>),
    LayerNorm {
        epsilon: f32,
        affine: Option<(Vec<f32>, Vec<f32>)>,
    },
    Embedding(Tensor),
    Attention(Attention),
    Transformer(Vec<Graph>),
    Gated(Gated),
    Conditional(Conditional),
    Filterbank(Filterbank),
}

/// Limits apply cumulatively to one inference, including all subgraphs.
#[derive(Debug)]
pub struct Budget {
    pub multiply_adds: usize,
    pub output_elements: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            multiply_adds: 250_000_000,
            output_elements: 64 * 1024 * 1024,
        }
    }
}
impl Budget {
    pub(crate) fn spend(&mut self, macs: usize, elements: usize) -> Result<()> {
        self.multiply_adds = self
            .multiply_adds
            .checked_sub(macs)
            .ok_or_else(|| invalid("inference arithmetic budget exceeded"))?;
        self.output_elements = self
            .output_elements
            .checked_sub(elements)
            .ok_or_else(|| invalid("inference allocation budget exceeded"))?;
        Ok(())
    }
}

impl Graph {
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        match self {
            Self::Sequential(nodes) | Self::Transformer(nodes) => {
                m::nested(nodes, Self::estimated_bytes)
            }
            Self::Affine(value) => value.estimated_bytes(),
            Self::Activation(_) | Self::GeneratedActivation(_) => 0,
            Self::Convolution(value) => value.estimated_bytes(),
            Self::Gru(value) => value.estimated_bytes(),
            Self::BidirectionalGru(a, b) => m::sum([a.estimated_bytes(), b.estimated_bytes()]),
            Self::ChannelAffine(a, b) => m::sum([m::values(a), m::values(b)]),
            Self::LayerNorm { affine, .. } => {
                m::optional(affine, |(a, b)| m::sum([m::values(a), m::values(b)]))
            }
            Self::Embedding(value) => value.estimated_bytes(),
            Self::Attention(value) => value.estimated_bytes(),
            Self::Gated(value) => value.estimated_bytes(),
            Self::Conditional(value) => value.estimated_bytes(),
            Self::Filterbank(value) => value.estimated_bytes(),
        }
    }

    pub fn compile(n: &Node<'_>) -> Result<Self> {
        let g = match n.tag.as_str() {
            "modm0" => {
                payload(n, 0)?;
                Self::Sequential(
                    n.children
                        .iter()
                        .map(Self::compile)
                        .collect::<Result<_>>()?,
                )
            }
            "modl0" => Self::Affine(Affine::from_node(n)?),
            "moda1" | "moda2" => {
                payload(n, 0)?;
                children(n, &[0])?;
                let kind = if n.tag == "moda1" {
                    GeneratedActivationKind::Tanh
                } else {
                    GeneratedActivationKind::Sigmoid
                };
                Self::GeneratedActivation(GeneratedActivation::new(kind)?)
            }
            "moda0" | "moda5" | "moda7" => {
                payload(n, 0)?;
                children(n, &[0])?;
                Self::Activation(match n.tag.as_str() {
                    "moda0" => Activation::Relu,
                    "moda7" => Activation::Silu,
                    _ => Activation::Identity,
                })
            }
            "moda3" => {
                payload(n, 4)?;
                children(n, &[0])?;
                let slope = f32::from_bits(u32_at(n.payload, 0)?);
                if !slope.is_finite() {
                    return Err(invalid("non-finite leaky ReLU slope"));
                }
                Self::Activation(Activation::LeakyRelu(slope))
            }
            "modl1" | "modl2" => Self::Convolution(Convolution::from_node(n)?),
            "modl3" => Self::Gru(Gru::from_node(n)?),
            "modl6" => {
                payload(n, 0)?;
                children(n, &[2])?;
                if n.children.iter().any(|c| c.tag != "modl3") {
                    return Err(invalid("bidirectional children must be GRUs"));
                }
                let a = Gru::from_node(&n.children[0])?;
                let b = Gru::from_node(&n.children[1])?;
                if a.input_channels() != b.input_channels() {
                    return Err(invalid("bidirectional input dimensions differ"));
                }
                Self::BidirectionalGru(a, b)
            }
            "modl4" => {
                payload(n, 0)?;
                children(n, &[1])?;
                let table = matrix(&n.children[0])?;
                if table.rows() == 0 {
                    return Err(invalid("empty embedding table"));
                }
                Self::Embedding(table)
            }
            "modl5" => {
                payload(n, 0)?;
                children(n, &[2])?;
                let gain = vector(&n.children[0])?;
                let bias = vector(&n.children[1])?;
                if gain.len() != bias.len() {
                    return Err(invalid("channel affine dimensions differ"));
                }
                Self::ChannelAffine(gain, bias)
            }
            "modl8" => {
                payload(n, 5)?;
                let epsilon = f32::from_bits(u32_at(n.payload, 0)?);
                if !epsilon.is_finite() || epsilon <= 0.0 || n.payload[4] > 1 {
                    return Err(invalid("invalid layer norm configuration"));
                }
                let affine = if n.payload[4] == 1 {
                    children(n, &[2])?;
                    let gain = vector(&n.children[0])?;
                    let bias = vector(&n.children[1])?;
                    if gain.len() != bias.len() {
                        return Err(invalid("layer norm dimensions differ"));
                    }
                    Some((gain, bias))
                } else {
                    children(n, &[0])?;
                    None
                };
                Self::LayerNorm { epsilon, affine }
            }
            "modl7" => Self::Attention(Attention::from_node(n)?),
            "modl10" => {
                payload(n, 1)?;
                children(n, &[6])?;
                if n.payload[0] != 0
                    || n.children[0].tag != "modl7"
                    || n.children[1].tag != "modl0"
                    || n.children[2].tag != "modl0"
                    || n.children[3].tag != "modl8"
                    || n.children[4].tag != "modl8"
                {
                    return Err(invalid("unsupported transformer configuration"));
                }
                Self::Transformer(
                    n.children
                        .iter()
                        .map(Self::compile)
                        .collect::<Result<_>>()?,
                )
            }
            "_gnc1v0" => Self::Gated(Gated::from_node(n)?),
            "_ncwnv0" => Self::Conditional(Conditional::from_node(n)?),
            "_ppdsv0" | "_ppusv0" => Self::Filterbank(Filterbank::from_node(n)?),
            other => {
                return Err(invalid(format!(
                    "unsupported graph tag {other} at {}",
                    n.offset
                )));
            }
        };
        Ok(g)
    }

    /// Every call starts recurrent nodes from zero. No persistent hidden state.
    pub fn run(&self, input: &Tensor) -> Result<Tensor> {
        self.run_with(input, &mut Budget::default())
    }

    pub fn run_with(&self, x: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        self.run_optional(x, None, budget)
    }
    /// Continue a single GRU with caller-owned hidden state, updated only on success.
    pub fn recurrent_state_len(&self) -> Result<usize> {
        match self {
            Self::Gru(g) => Ok(g.hidden_channels()),
            _ => Err(invalid("persistent state requires a single GRU")),
        }
    }
    pub fn run_recurrent(
        &self,
        x: &Tensor,
        state: &mut [f32],
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let timing = profiling::enter("gru_state", x.rows(), x.cols(), budget.multiply_adds);
        let result = match self {
            Self::Gru(g) => g.run_state(x, false, state, budget),
            _ => Err(invalid("persistent state requires a single GRU")),
        };
        if let Some(timing) = timing {
            timing.finish(
                result
                    .as_ref()
                    .ok()
                    .map(|output| (output.rows(), output.cols())),
                budget.multiply_adds,
            );
        }
        result
    }
    pub fn run_conditioned(
        &self,
        x: &Tensor,
        condition: &Tensor,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        self.run_optional(x, Some(condition), budget)
    }
    fn run_optional(
        &self,
        x: &Tensor,
        condition: Option<&Tensor>,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let timing = profiling::enter(
            self.operator_name(),
            x.rows(),
            x.cols(),
            budget.multiply_adds,
        );
        // Keep every ordinary early-return/error inside this call so the span
        // can observe the remaining budget on both Result paths.
        let result = self.run_optional_inner(x, condition, budget);
        if let Some(timing) = timing {
            timing.finish(
                result
                    .as_ref()
                    .ok()
                    .map(|output| (output.rows(), output.cols())),
                budget.multiply_adds,
            );
        }
        result
    }

    fn run_optional_inner(
        &self,
        x: &Tensor,
        condition: Option<&Tensor>,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let y = match self {
            Self::Sequential(nodes) => {
                budget.spend(0, x.values().len())?;
                let mut y = x.clone();
                for node in nodes {
                    y = node.run_optional(&y, condition, budget)?;
                }
                y
            }
            Self::Affine(a) => a.run(x, budget)?,
            Self::Activation(a) => {
                budget.spend(x.values().len().saturating_mul(32), x.values().len())?;
                Tensor::new(
                    x.rows(),
                    x.cols(),
                    x.values().iter().map(|v| a.apply(*v)).collect(),
                )?
            }
            Self::GeneratedActivation(a) => a.run(x, budget)?,
            Self::Convolution(c) => c.run(x, budget)?,
            Self::Gru(g) => g.run(x, false, budget)?,
            Self::BidirectionalGru(a, b) => {
                let f = a.run(x, false, budget)?;
                let r = b.run(x, true, budget)?;
                let cols = f.cols() + r.cols();
                budget.spend(0, x.rows().saturating_mul(cols))?;
                let mut out = Tensor::zeros(x.rows(), cols)?;
                for t in 0..x.rows() {
                    out.row_mut(t)[..f.cols()].copy_from_slice(f.row(t));
                    out.row_mut(t)[f.cols()..].copy_from_slice(r.row(t));
                }
                out
            }
            Self::ChannelAffine(g, b) => {
                if x.cols() != g.len()
                    || x.cols() != b.len()
                    || g.iter().chain(b).any(|v| !v.is_finite())
                {
                    return Err(invalid("channel affine input dimension"));
                }
                budget.spend(x.values().len(), x.values().len())?;
                let values = x
                    .values()
                    .iter()
                    .enumerate()
                    .map(|(i, v)| v * g[i % x.cols()] + b[i % x.cols()])
                    .collect();
                Tensor::new(x.rows(), x.cols(), values)?
            }
            Self::LayerNorm { epsilon, affine } => {
                if !epsilon.is_finite()
                    || *epsilon <= 0.0
                    || affine.as_ref().is_some_and(|(g, b)| {
                        g.len() != x.cols()
                            || b.len() != x.cols()
                            || g.iter().chain(b).any(|v| !v.is_finite())
                    })
                {
                    return Err(invalid("layer norm input dimension"));
                }
                budget.spend(x.values().len().saturating_mul(4), x.values().len())?;
                let mut y = Tensor::zeros(x.rows(), x.cols())?;
                for t in 0..x.rows() {
                    let row = x.row(t);
                    let mean = row.iter().copied().sum::<f32>() / x.cols() as f32;
                    let var = row
                        .iter()
                        .map(|v| {
                            let d = v - mean;
                            d * d
                        })
                        .sum::<f32>()
                        / x.cols() as f32;
                    let denom = (var + epsilon).sqrt();
                    for c in 0..x.cols() {
                        let v = (row[c] - mean) / denom;
                        y.row_mut(t)[c] = if let Some((g, b)) = affine {
                            v * g[c] + b[c]
                        } else {
                            v
                        };
                    }
                }
                y
            }
            Self::Embedding(table) => {
                if x.cols() != 1 {
                    return Err(invalid("embedding expects one index per row"));
                }
                budget.spend(0, x.rows().saturating_mul(table.rows()))?;
                let mut y = Tensor::zeros(x.rows(), table.rows())?;
                for t in 0..x.rows() {
                    let index = x.row(t)[0].round().clamp(0.0, (table.cols() - 1) as f32) as usize;
                    for c in 0..table.rows() {
                        y.row_mut(t)[c] = table.row(c)[index];
                    }
                }
                y
            }
            Self::Attention(a) => a.run(x, x, x, budget)?,
            Self::Transformer(nodes) => {
                if nodes.len() != 6 {
                    return Err(invalid("transformer children"));
                }
                let attention = nodes[0].run_with(x, budget)?;
                let first = add(x, &attention, budget)?;
                let normalized = nodes[3].run_with(&first, budget)?;
                let h = nodes[1].run_with(&normalized, budget)?;
                let h = nodes[5].run_with(&h, budget)?;
                let h = nodes[2].run_with(&h, budget)?;
                nodes[4].run_with(&add(&normalized, &h, budget)?, budget)?
            }
            Self::Gated(g) => g.run(x, condition, budget)?,
            Self::Conditional(c) => c.run(x, condition, budget)?,
            Self::Filterbank(f) => f.run(x, budget)?,
        };
        y.ensure_finite()?;
        Ok(y)
    }

    fn operator_name(&self) -> &'static str {
        match self {
            Self::Sequential(_) => "sequential",
            Self::Affine(_) => "affine",
            Self::Activation(_) | Self::GeneratedActivation(_) => "activation",
            Self::Convolution(layer) => layer.operator_name(),
            Self::Gru(_) => "gru",
            Self::BidirectionalGru(_, _) => "bidirectional_gru",
            Self::ChannelAffine(_, _) => "channel_affine",
            Self::LayerNorm { .. } => "layer_norm",
            Self::Embedding(_) => "embedding",
            Self::Attention(_) => "attention",
            Self::Transformer(_) => "transformer",
            Self::Gated(_) => "gated",
            Self::Conditional(_) => "conditional",
            Self::Filterbank(_) => "filterbank",
        }
    }
}

#[derive(Debug)]
pub struct Affine {
    pub(crate) weight: Tensor,
    pub(crate) bias: Option<Vec<f32>>,
}
impl Affine {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.weight.estimated_bytes(),
            crate::memory::optional(&self.bias, crate::memory::values),
        ])
    }

    pub(crate) fn from_node(n: &Node<'_>) -> Result<Self> {
        payload(n, 0)?;
        children(n, &[1, 2])?;
        Self::new(
            matrix(&n.children[0])?,
            if n.children.len() == 2 {
                Some(vector(&n.children[1])?)
            } else {
                None
            },
        )
    }
    pub fn new(weight: Tensor, bias: Option<Vec<f32>>) -> Result<Self> {
        if weight.rows() == 0
            || bias
                .as_ref()
                .is_some_and(|b| b.len() != weight.rows() || b.iter().any(|v| !v.is_finite()))
        {
            return Err(invalid("invalid affine bias/dimensions"));
        }
        Ok(Self { weight, bias })
    }
    pub(crate) fn dot(&self, row: &[f32], out: usize) -> f32 {
        dot(self.weight.row(out), row)
    }
    pub(crate) fn run(&self, x: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        if x.cols() != self.weight.cols() {
            return Err(invalid("affine input dimension"));
        }
        budget.spend(
            x.rows().saturating_mul(self.weight.values().len()),
            x.rows().saturating_mul(self.weight.rows()),
        )?;
        let mut y = Tensor::zeros(x.rows(), self.weight.rows())?;
        let outputs = self.weight.rows();
        parallel::for_rows(
            y.values_mut(),
            outputs,
            self.weight.values().len(),
            |first, target| {
                for (offset, row) in target.chunks_exact_mut(outputs).enumerate() {
                    let input = x.row(first + offset);
                    let grouped = outputs / 4 * 4;
                    for o in (0..grouped).step_by(4) {
                        let sums = dot4(
                            [
                                self.weight.row(o),
                                self.weight.row(o + 1),
                                self.weight.row(o + 2),
                                self.weight.row(o + 3),
                            ],
                            input,
                        );
                        for lane in 0..4 {
                            row[o + lane] =
                                sums[lane] + self.bias.as_ref().map_or(0.0, |b| b[o + lane]);
                        }
                    }
                    for o in grouped..outputs {
                        row[o] = self.dot(input, o) + self.bias.as_ref().map_or(0.0, |b| b[o]);
                    }
                }
            },
        );
        Ok(y)
    }
}

/// Float32 operands accumulate in float64 before rounding the dot result.
pub(crate) fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(a, b)| f64::from(*a) * f64::from(*b))
        .sum::<f64>() as f32
}

// Interleave independent outputs while retaining each input-order reduction.
pub(crate) fn dot4(rows: [&[f32]; 4], input: &[f32]) -> [f32; 4] {
    let zero = std::iter::empty::<f64>().sum::<f64>();
    let (mut sum0, mut sum1, mut sum2, mut sum3) = (zero, zero, zero, zero);
    for ((((w0, w1), w2), w3), input) in rows[0]
        .iter()
        .zip(rows[1])
        .zip(rows[2])
        .zip(rows[3])
        .zip(input)
    {
        let input = f64::from(*input);
        sum0 += f64::from(*w0) * input;
        sum1 += f64::from(*w1) * input;
        sum2 += f64::from(*w2) * input;
        sum3 += f64::from(*w3) * input;
    }
    [sum0 as f32, sum1 as f32, sum2 as f32, sum3 as f32]
}

pub(crate) fn payload(n: &Node<'_>, len: usize) -> Result<()> {
    if n.payload.len() != len {
        Err(invalid(format!("{} payload must be {len} bytes", n.tag)))
    } else {
        Ok(())
    }
}
pub(crate) fn children(n: &Node<'_>, counts: &[usize]) -> Result<()> {
    if !counts.contains(&n.children.len()) {
        Err(invalid(format!("{} unexpected child count", n.tag)))
    } else {
        Ok(())
    }
}
pub(crate) fn matrix(n: &Node<'_>) -> Result<Tensor> {
    if !matches!(n.tag.as_str(), "prim0" | "prim4") {
        return Err(invalid("expected float32 or explicitly dequantized matrix"));
    }
    n.tensor()
}

pub(crate) fn add(a: &Tensor, b: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    if a.rows() != b.rows() || a.cols() != b.cols() {
        return Err(invalid("residual dimensions differ"));
    }
    budget.spend(a.values().len(), a.values().len())?;
    Tensor::new(
        a.rows(),
        a.cols(),
        a.values()
            .iter()
            .zip(b.values())
            .map(|(a, b)| a + b)
            .collect(),
    )
}
pub(crate) fn vector(n: &Node<'_>) -> Result<Vec<f32>> {
    if n.tag != "prim1" {
        return Err(invalid("expected prim1 vector"));
    }
    Ok(n.tensor()?.values().to_vec())
}
