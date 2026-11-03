use super::{Budget, children, payload, vector};
use crate::{Result, dnni::Node, invalid, tensor::Tensor};

const BANDS: usize = 8;
const TAPS: usize = 65;

#[derive(Debug)]
pub struct Filterbank {
    filters: Tensor,
    synthesis: bool,
}
impl Filterbank {
    pub fn estimated_bytes(&self) -> usize {
        self.filters.estimated_bytes()
    }

    pub(super) fn from_node(n: &Node<'_>) -> Result<Self> {
        payload(n, 0)?;
        children(n, &[BANDS])?;
        let filters = n.children.iter().map(vector).collect::<Result<Vec<_>>>()?;
        if filters.iter().any(|v| v.len() != TAPS) {
            return Err(invalid("PQMF filter length"));
        }
        Ok(Self {
            filters: Tensor::new(BANDS, TAPS, filters.into_iter().flatten().collect())?,
            synthesis: n.tag == "_ppusv0",
        })
    }
    pub(super) fn run(&self, x: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        if self.synthesis {
            crate::pqmf::synthesis(x, &self.filters, budget)
        } else {
            crate::pqmf::analysis(x, &self.filters, budget)
        }
    }
}
