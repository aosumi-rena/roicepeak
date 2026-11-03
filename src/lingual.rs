//! Learned phoneme/language embeddings with explicit one-hot linguistic input.
use crate::{
    Result,
    dnni::Node,
    graph::{Budget, Graph},
    invalid,
    sequence::concatenate,
    tensor::{Tensor, u32_at},
};
pub struct Lingual {
    pub feature_channels: usize,
    phones: Graph,
    languages: Graph,
    projection: Graph,
}
impl Lingual {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.phones.estimated_bytes(),
            self.languages.estimated_bytes(),
            self.projection.estimated_bytes(),
        ])
    }

    pub fn compile(n: &Node<'_>) -> Result<Self> {
        if n.tag != "_mlien0" || n.payload.len() != 8 || n.children.len() != 3 {
            return Err(invalid("lingual encoder structure"));
        }
        Ok(Self {
            feature_channels: u32_at(n.payload, 0)? as usize,
            phones: Graph::compile(&n.children[0])?,
            languages: Graph::compile(&n.children[1])?,
            projection: Graph::compile(&n.children[2])?,
        })
    }
    /// Input columns: phone ID, language ID, then the model's feature channels.
    pub fn run(&self, input: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        if input.cols() != self.feature_channels + 2 {
            return Err(invalid("lingual input width"));
        }
        let mut phones = Tensor::zeros(input.rows(), 1)?;
        let mut languages = phones.clone();
        let mut features = Tensor::zeros(input.rows(), self.feature_channels)?;
        for t in 0..input.rows() {
            let r = input.row(t);
            if r[..2].iter().any(|v| *v < 0.0 || v.fract() != 0.0) {
                return Err(invalid("phone/language IDs must be nonnegative integers"));
            }
            phones.row_mut(t)[0] = r[0];
            languages.row_mut(t)[0] = r[1];
            features.row_mut(t).copy_from_slice(&r[2..]);
        }
        let phones = self.phones.run_with(&phones, budget)?;
        let languages = self.languages.run_with(&languages, budget)?;
        self.projection.run_with(
            &concatenate(&[&phones, &features, &languages], budget)?,
            budget,
        )
    }
}
