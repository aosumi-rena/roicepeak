use super::{Affine, Budget, children, matrix, payload, vector};
use crate::{Result, activation::sigmoid, dnni::Node, invalid, tensor::Tensor};

/// Reset-after GRU: input r,z,n then recurrent r,z,n; reset gates recurrent bias.
#[derive(Debug)]
pub struct Gru {
    affines: Vec<Affine>,
    hidden: usize,
    inputs: usize,
}
impl Gru {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::nested(&self.affines, Affine::estimated_bytes)
    }

    pub(super) fn hidden_channels(&self) -> usize {
        self.hidden
    }
    pub(super) fn from_node(n: &Node<'_>) -> Result<Self> {
        payload(n, 0)?;
        children(n, &[12])?;
        let mut a = Vec::new();
        for p in n.children.chunks_exact(2) {
            a.push(Affine::new(matrix(&p[0])?, Some(vector(&p[1])?))?);
        }
        let hidden = a[0].weight.rows();
        let inputs = a[0].weight.cols();
        for (i, g) in a.iter().enumerate() {
            if g.weight.rows() != hidden || g.weight.cols() != if i < 3 { inputs } else { hidden } {
                return Err(invalid("GRU affine dimensions differ"));
            }
        }
        Ok(Self {
            affines: a,
            hidden,
            inputs,
        })
    }
    pub(super) fn input_channels(&self) -> usize {
        self.inputs
    }
    pub(super) fn run(&self, x: &Tensor, reverse: bool, budget: &mut Budget) -> Result<Tensor> {
        let mut state = vec![0.0; self.hidden];
        self.run_state(x, reverse, &mut state, budget)
    }
    pub(super) fn run_state(
        &self,
        x: &Tensor,
        reverse: bool,
        state: &mut [f32],
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if x.cols() != self.inputs {
            return Err(invalid("GRU input dimension"));
        }
        if state.len() != self.hidden || state.iter().any(|v| !v.is_finite()) {
            return Err(invalid("GRU hidden-state dimension or values"));
        }
        let macs = x
            .rows()
            .saturating_mul(self.hidden)
            .saturating_mul(self.inputs.saturating_add(self.hidden))
            .saturating_mul(3);
        budget.spend(macs, x.rows().saturating_add(2).saturating_mul(self.hidden))?;
        let mut y = Tensor::zeros(x.rows(), self.hidden)?;
        let mut h = state.to_vec();
        let mut next = h.clone();
        let a = &self.affines;
        for step in 0..x.rows() {
            let t = if reverse { x.rows() - 1 - step } else { step };
            for o in 0..self.hidden {
                let bias = |i: usize| a[i].bias.as_ref().unwrap()[o];
                let gate = |i: usize| {
                    sigmoid((a[i].dot(x.row(t), o) + (bias(i) + bias(i + 3))) + a[i + 3].dot(&h, o))
                };
                let r = gate(0);
                let z = gate(1);
                let recurrent = a[5].dot(&h, o) + bias(5);
                let input = a[2].dot(x.row(t), o) + bias(2);
                let candidate = (recurrent * r + input).tanh();
                next[o] = h[o] * z + candidate * (1.0 - z);
            }
            h.copy_from_slice(&next);
            y.row_mut(t).copy_from_slice(&h);
        }
        y.ensure_finite()?;
        state.copy_from_slice(&h);
        Ok(y)
    }
}
