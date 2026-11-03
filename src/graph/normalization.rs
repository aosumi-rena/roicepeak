use super::{Budget, children, payload, vector};
use crate::{
    Result,
    dnni::Node,
    invalid, memory,
    tensor::{Tensor, u32_at},
};

#[derive(Clone, Copy, Debug)]
enum Scope {
    Row,
    Global,
}

#[derive(Debug)]
enum Affine {
    Vector(Vec<f32>, Vec<f32>),
    Matrix(Tensor, Tensor),
}

#[derive(Debug)]
pub struct Normalization {
    scope: Scope,
    epsilon: f32,
    affine: Option<Affine>,
}

impl Normalization {
    pub(super) fn from_node(node: &Node<'_>) -> Result<Self> {
        payload(node, 5)?;
        let epsilon = f32::from_bits(u32_at(node.payload, 0)?);
        if !epsilon.is_finite() || epsilon <= 0.0 || node.payload[4] > 1 {
            return Err(invalid("invalid normalization configuration"));
        }
        let scope = match node.tag.as_str() {
            "modl8" => Scope::Row,
            "modl9" => Scope::Global,
            _ => return Err(invalid("unsupported normalization")),
        };
        let affine = if node.payload[4] == 0 {
            children(node, &[0])?;
            None
        } else {
            children(node, &[2])?;
            Some(match scope {
                Scope::Row => {
                    let gain = vector(&node.children[0])?;
                    let bias = vector(&node.children[1])?;
                    if gain.len() != bias.len() {
                        return Err(invalid("normalization dimensions differ"));
                    }
                    Affine::Vector(gain, bias)
                }
                Scope::Global => {
                    if node.children.iter().any(|child| child.tag != "prim0") {
                        return Err(invalid("global normalization requires float matrices"));
                    }
                    let gain = node.children[0].tensor()?;
                    let bias = node.children[1].tensor()?;
                    if gain.rows() != bias.rows() || gain.cols() != bias.cols() {
                        return Err(invalid("normalization dimensions differ"));
                    }
                    Affine::Matrix(gain, bias)
                }
            })
        };
        Ok(Self {
            scope,
            epsilon,
            affine,
        })
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        memory::optional(&self.affine, |affine| match affine {
            Affine::Vector(gain, bias) => memory::sum([memory::values(gain), memory::values(bias)]),
            Affine::Matrix(gain, bias) => {
                memory::sum([gain.estimated_bytes(), bias.estimated_bytes()])
            }
        })
    }

    pub(super) fn kind(&self) -> &'static str {
        match self.scope {
            Scope::Row => "layer_norm",
            Scope::Global => "global_norm",
        }
    }

    pub(super) fn run(&self, input: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        if self.affine.as_ref().is_some_and(|affine| match affine {
            Affine::Vector(gain, bias) => gain.len() != input.cols() || bias.len() != input.cols(),
            Affine::Matrix(gain, bias) => {
                gain.rows() != input.rows()
                    || bias.rows() != input.rows()
                    || gain.cols() != input.cols()
                    || bias.cols() != input.cols()
            }
        }) {
            return Err(invalid("normalization input dimensions differ"));
        }
        let elements = input.values().len();
        let groups = if elements == 0 {
            0
        } else {
            match self.scope {
                Scope::Row => input.rows(),
                Scope::Global => 1,
            }
        };
        let per_element = if self.affine.is_some() {
            7usize
        } else {
            5usize
        };
        let work = elements
            .checked_mul(per_element)
            .and_then(|value| {
                groups
                    .checked_mul(6)
                    .and_then(|extra| value.checked_add(extra))
            })
            .ok_or_else(|| invalid("normalization work overflow"))?;
        budget.spend(work, elements)?;
        let mut output = Tensor::zeros(input.rows(), input.cols())?;
        if elements == 0 {
            return Ok(output);
        }
        match self.scope {
            Scope::Row => {
                for row in 0..input.rows() {
                    let (mean, denominator) = statistics(input.row(row), self.epsilon)?;
                    for column in 0..input.cols() {
                        let normalized = (input.row(row)[column] - mean) / denominator;
                        output.row_mut(row)[column] = self.affine_value(row, column, normalized);
                    }
                }
            }
            Scope::Global => {
                let (mean, denominator) = statistics(input.values(), self.epsilon)?;
                for row in 0..input.rows() {
                    for column in 0..input.cols() {
                        let normalized = (input.row(row)[column] - mean) / denominator;
                        output.row_mut(row)[column] = self.affine_value(row, column, normalized);
                    }
                }
            }
        }
        Ok(output)
    }

    fn affine_value(&self, row: usize, column: usize, normalized: f32) -> f32 {
        match &self.affine {
            None => normalized,
            Some(Affine::Vector(gain, bias)) => gain[column] * normalized + bias[column],
            Some(Affine::Matrix(gain, bias)) => {
                gain.row(row)[column] * normalized + bias.row(row)[column]
            }
        }
    }
}

fn statistics(values: &[f32], epsilon: f32) -> Result<(f32, f32)> {
    let count =
        i32::try_from(values.len()).map_err(|_| invalid("normalization count overflow"))? as f32;
    let mut sum = 0.0f32;
    let mut sum_squares = 0.0f32;
    for &value in values {
        sum += value;
        let square = value * value;
        sum_squares += square;
    }
    let mean = sum / count;
    let second_moment = sum_squares / count;
    let mean_square = mean * mean;
    let variance = second_moment - mean_square;
    Ok((mean, (variance + epsilon).sqrt()))
}
