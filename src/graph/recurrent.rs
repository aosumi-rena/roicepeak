//! Ordered GRU stages over retained matrix kernels.
use super::generated_activation::{GeneratedActivation, Kind};
use super::matrix_storage::{Matrix, Workspace};
use super::{Budget, children, payload, vector};
use crate::{Result, dnni::Node, invalid, tensor::Tensor};

#[derive(Debug)]
pub struct Gru {
    matrices: Vec<Matrix>,
    biases: Vec<Vec<f32>>,
    reset_bias: Vec<f32>,
    update_bias: Vec<f32>,
    sigmoid: GeneratedActivation,
    tanh: GeneratedActivation,
    hidden: usize,
    inputs: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(super) enum Stage {
    OldHidden = 0,
    ResetSeed = 1,
    ResetInputAdded = 2,
    ResetRecurrentAdded = 3,
    ResetActivated = 4,
    UpdateSeed = 5,
    UpdateInputAdded = 6,
    UpdateRecurrentAdded = 7,
    UpdateActivated = 8,
    CandidateSeed = 9,
    CandidateRecurrentAdded = 10,
    CandidateResetMultiplied = 11,
    CandidateInputAdded = 12,
    CandidateInputBiasAdded = 13,
    CandidateActivated = 14,
    HiddenMultipliedUpdate = 15,
    UpdateComplemented = 16,
    CandidateWeightedComplement = 17,
    NewHidden = 18,
}

impl Gru {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            crate::memory::nested(&self.matrices, Matrix::estimated_bytes),
            crate::memory::nested(&self.biases, crate::memory::values),
            crate::memory::values(&self.reset_bias),
            crate::memory::values(&self.update_bias),
        ])
    }
    pub(super) fn hidden_channels(&self) -> usize {
        self.hidden
    }
    pub(super) fn input_channels(&self) -> usize {
        self.inputs
    }
    pub(super) fn from_node(node: &Node<'_>) -> Result<Self> {
        payload(node, 0)?;
        children(node, &[12])?;
        let primitive = node.children[0].tag.as_str();
        if !matches!(primitive, "prim0" | "prim4")
            || node
                .children
                .chunks_exact(2)
                .any(|pair| pair[0].tag != primitive)
        {
            return Err(invalid("unsupported GRU matrix primitive combination"));
        }
        let mut matrices = Vec::with_capacity(6);
        let mut biases = Vec::with_capacity(6);
        for pair in node.children.chunks_exact(2) {
            matrices.push(Matrix::from_node(&pair[0])?.into_product_kernel()?);
            biases.push(vector(&pair[1])?);
        }
        let hidden = matrices[0].rows();
        let inputs = matrices[0].cols();
        if hidden == 0
            || inputs == 0
            || matrices.iter().enumerate().any(|(index, matrix)| {
                matrix.rows() != hidden || matrix.cols() != if index < 3 { inputs } else { hidden }
            })
            || biases.iter().any(|bias| bias.len() != hidden)
        {
            return Err(invalid("retained GRU matrix/bias dimensions differ"));
        }
        let reset_bias = biases[3]
            .iter()
            .zip(&biases[0])
            .map(|(r, w)| r + w)
            .collect::<Vec<_>>();
        let update_bias = biases[4]
            .iter()
            .zip(&biases[1])
            .map(|(r, w)| r + w)
            .collect::<Vec<_>>();
        if reset_bias
            .iter()
            .chain(&update_bias)
            .any(|v| !v.is_finite())
        {
            return Err(invalid("nonfinite combined GRU bias"));
        }
        Ok(Self {
            matrices,
            biases,
            reset_bias,
            update_bias,
            sigmoid: GeneratedActivation::new(Kind::Sigmoid)?,
            tanh: GeneratedActivation::new(Kind::Tanh)?,
            hidden,
            inputs,
        })
    }
    pub(super) fn run(&self, input: &Tensor, reverse: bool, budget: &mut Budget) -> Result<Tensor> {
        if input.cols() != self.inputs {
            return Err(invalid("GRU input dimension"));
        }
        if input.rows() == 0 {
            return Tensor::zeros(0, self.hidden);
        }
        budget.spend(0, self.hidden)?;
        let mut state = vec![0.0; self.hidden];
        self.run_state(input, reverse, &mut state, budget)
    }
    pub(super) fn run_state(
        &self,
        input: &Tensor,
        reverse: bool,
        state: &mut [f32],
        budget: &mut Budget,
    ) -> Result<Tensor> {
        self.run_state_observed(input, reverse, state, budget, |_, _, _| Ok(()))
    }
    pub(super) fn run_state_observed(
        &self,
        input: &Tensor,
        reverse: bool,
        state: &mut [f32],
        budget: &mut Budget,
        mut observe: impl FnMut(usize, Stage, &[f32]) -> Result<()>,
    ) -> Result<Tensor> {
        if input.cols() != self.inputs {
            return Err(invalid("GRU input dimension"));
        }
        if state.len() != self.hidden || state.iter().any(|v| !v.is_finite()) {
            return Err(invalid("GRU hidden-state dimension or values"));
        }
        if input.rows() == 0 {
            return Tensor::zeros(0, self.hidden);
        }
        let quantized = self.matrices[0].is_quantized();
        let products = self.matrices.iter().fold(0usize, |total, matrix| {
            total.saturating_add(matrix.elements())
        });
        let quantization = if quantized {
            self.inputs.saturating_add(self.hidden).saturating_mul(3)
        } else {
            0
        };
        let vector_and_activation = self.hidden.saturating_mul(6 + 3 * 32);
        let matrix_workspace = if quantized {
            self.inputs
                .max(self.hidden)
                .max(4)
                .saturating_add(self.hidden.max(4))
        } else {
            0
        };
        let scratch = self
            .hidden
            .saturating_mul(4)
            .saturating_add(matrix_workspace);
        budget.spend(
            input.rows().saturating_mul(
                products
                    .saturating_add(quantization)
                    .saturating_add(vector_and_activation),
            ),
            input
                .rows()
                .saturating_mul(self.hidden)
                .saturating_add(scratch),
        )?;
        let mut output = Tensor::zeros(input.rows(), self.hidden)?;
        let mut hidden = state.to_vec();
        let mut reset = vec![0.0; self.hidden];
        let mut update = vec![0.0; self.hidden];
        let mut candidate = vec![0.0; self.hidden];
        let mut workspace = Workspace::default();
        let add = Matrix::add_mode();
        for step in 0..input.rows() {
            let time = if reverse {
                input.rows() - 1 - step
            } else {
                step
            };
            let source = input.row(time);
            observe(time, Stage::OldHidden, &hidden)?;
            reset.copy_from_slice(&self.reset_bias);
            observe(time, Stage::ResetSeed, &reset)?;
            self.matrices[0].product_into(source, &mut reset, add, &mut workspace)?;
            observe(time, Stage::ResetInputAdded, &reset)?;
            self.matrices[3].product_into(&hidden, &mut reset, add, &mut workspace)?;
            observe(time, Stage::ResetRecurrentAdded, &reset)?;
            self.sigmoid.apply_in_place(&mut reset)?;
            observe(time, Stage::ResetActivated, &reset)?;

            update.copy_from_slice(&self.update_bias);
            observe(time, Stage::UpdateSeed, &update)?;
            self.matrices[1].product_into(source, &mut update, add, &mut workspace)?;
            observe(time, Stage::UpdateInputAdded, &update)?;
            self.matrices[4].product_into(&hidden, &mut update, add, &mut workspace)?;
            observe(time, Stage::UpdateRecurrentAdded, &update)?;
            self.sigmoid.apply_in_place(&mut update)?;
            observe(time, Stage::UpdateActivated, &update)?;

            candidate.copy_from_slice(&self.biases[5]);
            observe(time, Stage::CandidateSeed, &candidate)?;
            self.matrices[5].product_into(&hidden, &mut candidate, add, &mut workspace)?;
            observe(time, Stage::CandidateRecurrentAdded, &candidate)?;
            for (value, reset) in candidate.iter_mut().zip(&reset) {
                *value = *reset * *value;
            }
            observe(time, Stage::CandidateResetMultiplied, &candidate)?;
            self.matrices[2].product_into(source, &mut candidate, add, &mut workspace)?;
            observe(time, Stage::CandidateInputAdded, &candidate)?;
            for (value, bias) in candidate.iter_mut().zip(&self.biases[2]) {
                *value = *bias + *value;
            }
            observe(time, Stage::CandidateInputBiasAdded, &candidate)?;
            self.tanh.apply_in_place(&mut candidate)?;
            observe(time, Stage::CandidateActivated, &candidate)?;

            for (hidden, update) in hidden.iter_mut().zip(&update) {
                *hidden = *update * *hidden;
            }
            observe(time, Stage::HiddenMultipliedUpdate, &hidden)?;
            for value in &mut update {
                *value = 1.0 - *value;
            }
            observe(time, Stage::UpdateComplemented, &update)?;
            for (candidate, update) in candidate.iter_mut().zip(&update) {
                *candidate = *update * *candidate;
            }
            observe(time, Stage::CandidateWeightedComplement, &candidate)?;
            for (hidden, candidate) in hidden.iter_mut().zip(&candidate) {
                *hidden = *candidate + *hidden;
            }
            if hidden.iter().any(|v| !v.is_finite()) {
                return Err(invalid("nonfinite retained GRU hidden state"));
            }
            observe(time, Stage::NewHidden, &hidden)?;
            output.row_mut(time).copy_from_slice(&hidden);
        }
        output.ensure_finite()?;
        // Publish caller state only after all rows succeed.
        state.copy_from_slice(&hidden);
        Ok(output)
    }
}
