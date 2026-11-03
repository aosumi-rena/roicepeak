use super::{Affine, Budget, children, payload};
use crate::{
    Result,
    dnni::Node,
    invalid,
    tensor::{Tensor, u32_at},
};

#[derive(Debug)]
pub struct Attention {
    heads: usize,
    query: Affine,
    key: Affine,
    value: Affine,
    output: Affine,
}
impl Attention {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.query.estimated_bytes(),
            self.key.estimated_bytes(),
            self.value.estimated_bytes(),
            self.output.estimated_bytes(),
        ])
    }

    pub(super) fn from_node(n: &Node<'_>) -> Result<Self> {
        payload(n, 20)?;
        children(n, &[4])?;
        let qdim = u32_at(n.payload, 0)? as usize;
        let kdim = u32_at(n.payload, 4)? as usize;
        let vdim = u32_at(n.payload, 8)? as usize;
        let heads = u32_at(n.payload, 12)? as usize;
        // Only this configuration has been observed in the supplied plain LM.
        if n.payload[16..] != [1, 0, 1, 0] || heads == 0 {
            return Err(invalid("unsupported attention flags/head count"));
        }
        if n.children.iter().any(|c| c.tag != "modl0") {
            return Err(invalid("attention projections must be affine"));
        }
        let query = Affine::from_node(&n.children[0])?;
        let key = Affine::from_node(&n.children[1])?;
        let value = Affine::from_node(&n.children[2])?;
        let output = Affine::from_node(&n.children[3])?;
        if query.weight.cols() != qdim
            || key.weight.cols() != kdim
            || value.weight.cols() != vdim
            || query.weight.rows() != key.weight.rows()
            || query.weight.rows() % heads != 0
            || value.weight.rows() % heads != 0
            || output.weight.cols() != value.weight.rows()
        {
            return Err(invalid("attention projection dimensions"));
        }
        Ok(Self {
            heads,
            query,
            key,
            value,
            output,
        })
    }
    pub fn run(&self, q: &Tensor, k: &Tensor, v: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        if k.rows() != v.rows() || (q.rows() != 0 && k.rows() == 0) {
            return Err(invalid("attention key/value sequence dimensions"));
        }
        let q = self.query.run(q, budget)?;
        let k = self.key.run(k, budget)?;
        let v = self.value.run(v, budget)?;
        let dk = q.cols() / self.heads;
        let dv = v.cols() / self.heads;
        let work = q
            .rows()
            .saturating_mul(k.rows())
            .saturating_mul(dk.saturating_add(dv))
            .saturating_mul(self.heads);
        budget.spend(
            work,
            q.rows().saturating_mul(v.cols()).saturating_add(k.rows()),
        )?;
        let mut context = Tensor::zeros(q.rows(), v.cols())?;
        let mut scores = vec![0.0; k.rows()];
        let scale = 1.0 / (dk as f32).sqrt();
        for head in 0..self.heads {
            for t in 0..q.rows() {
                let mut maximum = f32::NEG_INFINITY;
                for (s, score) in scores.iter_mut().enumerate() {
                    let mut sum = 0.0;
                    for c in 0..dk {
                        sum += q.row(t)[head * dk + c] * k.row(s)[head * dk + c];
                    }
                    *score = sum * scale;
                    maximum = maximum.max(*score);
                }
                let mut total = 0.0;
                for score in &mut scores {
                    *score -= maximum;
                    total += score.exp();
                }
                let log_total = total.ln();
                for score in &mut scores {
                    *score = (*score - log_total).exp();
                }
                for c in 0..dv {
                    let mut sum = 0.0;
                    for (s, score) in scores.iter().enumerate() {
                        sum += score * v.row(s)[head * dv + c];
                    }
                    context.row_mut(t)[head * dv + c] = sum;
                }
            }
        }
        self.output.run(&context, budget)
    }
}
