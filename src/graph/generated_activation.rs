use super::Budget;
use crate::{Result, invalid, tensor::Tensor};

#[cfg(target_arch = "x86_64")]
#[path = "generated_activation_math.rs"]
mod math;

#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Tanh,
    Sigmoid,
}

#[derive(Debug)]
pub struct GeneratedActivation {
    kind: Kind,
    #[cfg(target_arch = "x86_64")]
    kernel: math::Kernel,
}

impl GeneratedActivation {
    pub(crate) fn new(kind: Kind) -> Result<Self> {
        #[cfg(target_arch = "x86_64")]
        {
            Ok(Self {
                kind,
                kernel: math::Kernel::new(math::Arithmetic::detected()),
            })
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = kind;
            Err(invalid(
                "generated Tanh/Sigmoid backend requires x86_64 SSE",
            ))
        }
    }

    pub(super) fn run(&self, input: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        #[cfg(target_arch = "x86_64")]
        {
            budget.spend(
                input.values().len().saturating_mul(32),
                input.values().len(),
            )?;
            let kind = match self.kind {
                Kind::Tanh => math::Kind::Tanh,
                Kind::Sigmoid => math::Kind::Sigmoid,
            };
            let mut output = Tensor::zeros(input.rows(), input.cols())?;
            for row in 0..input.rows() {
                self.kernel
                    .apply(kind, input.row(row), output.row_mut(row))
                    .map_err(|_| invalid("activation row length mismatch"))?;
            }
            Ok(output)
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = (input, budget, self.kind);
            Err(invalid(
                "generated Tanh/Sigmoid backend requires x86_64 SSE",
            ))
        }
    }
}

impl GeneratedActivation {
    /// Caller budgets a complete activation pass before allocating workspaces.
    pub(crate) fn apply_in_place(&self, values: &mut [f32]) -> Result<()> {
        #[cfg(target_arch = "x86_64")]
        {
            let kind = match self.kind {
                Kind::Tanh => math::Kind::Tanh,
                Kind::Sigmoid => math::Kind::Sigmoid,
            };
            self.kernel.apply_in_place(kind, values);
            if values.iter().any(|value| !value.is_finite()) {
                return Err(invalid("nonfinite generated activation result"));
            }
            Ok(())
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            let _ = (values, self.kind);
            Err(invalid(
                "generated Tanh/Sigmoid backend requires x86_64 SSE",
            ))
        }
    }
}
