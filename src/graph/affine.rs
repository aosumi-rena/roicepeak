use super::matrix_storage::{Accumulate, Workspace};
use super::{Budget, children, matrix_storage::Matrix, payload, vector};
use crate::{Result, dnni::Node, invalid, tensor::Tensor};

#[derive(Debug)]
pub struct Affine {
    pub(crate) weight: Matrix,
    pub(crate) bias: Option<Vec<f32>>,
}

impl Affine {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.weight.estimated_bytes(),
            crate::memory::optional(&self.bias, crate::memory::values),
        ])
    }
    pub(crate) fn from_node(node: &Node<'_>) -> Result<Self> {
        payload(node, 0)?;
        children(node, &[1, 2])?;
        Self::from_matrix(
            Matrix::from_node(&node.children[0])?,
            if node.children.len() == 2 {
                Some(vector(&node.children[1])?)
            } else {
                None
            },
        )
    }
    pub fn new(weight: Tensor, bias: Option<Vec<f32>>) -> Result<Self> {
        Self::from_matrix(Matrix::Dense(weight), bias)
    }
    fn from_matrix(weight: Matrix, bias: Option<Vec<f32>>) -> Result<Self> {
        if weight.rows() == 0
            || bias
                .as_ref()
                .is_some_and(|b| b.len() != weight.rows() || b.iter().any(|v| !v.is_finite()))
        {
            return Err(invalid("invalid affine bias/dimensions"));
        }
        Ok(Self {
            weight: weight.into_product_kernel()?,
            bias,
        })
    }
    pub(crate) fn run(&self, input: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        if input.cols() != self.weight.cols() {
            return Err(invalid("affine input dimension"));
        }
        let plan = super::parallel::FallibleRows::new(input.rows(), self.weight.elements());
        let scratch = self
            .weight
            .workspace_elements()
            .saturating_mul(plan.chunks());
        budget.spend(
            input.rows().saturating_mul(self.weight.elements()),
            input
                .rows()
                .saturating_mul(self.weight.rows())
                .saturating_add(scratch),
        )?;
        let mut output = Tensor::zeros(input.rows(), self.weight.rows())?;
        plan.execute(output.values_mut(), self.weight.rows(), |first, target| {
            let mut workspace = Workspace::default();
            for (offset, row) in target.chunks_exact_mut(self.weight.rows()).enumerate() {
                self.weight.product_into(
                    input.row(first + offset),
                    row,
                    Accumulate::Replace,
                    &mut workspace,
                )?;
                if let Some(bias) = &self.bias {
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
