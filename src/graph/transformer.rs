use super::{Attention, Budget, Graph, add, children, payload};
use crate::{Result, dnni::Node, invalid, tensor::Tensor};

#[derive(Debug)]
pub struct Transformer {
    attention: Attention,
    calculators: [Box<Graph>; 5],
    pre_normalize: bool,
}

impl Transformer {
    pub(super) fn from_node(node: &Node<'_>) -> Result<Self> {
        payload(node, 1)?;
        children(node, &[6])?;
        let [
            attention,
            first,
            second,
            norm_first,
            norm_second,
            activation,
        ] = node.children.as_slice()
        else {
            return Err(invalid("transformer children"));
        };
        if attention.tag != "modl7" {
            return Err(invalid("transformer requires attention as its first child"));
        }
        Ok(Self {
            attention: Attention::from_node(attention)?,
            calculators: [
                calculator(first)?,
                calculator(second)?,
                calculator(norm_first)?,
                calculator(norm_second)?,
                calculator(activation)?,
            ],
            pre_normalize: node.payload[0] & 1 != 0,
        })
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        crate::memory::sum(
            std::iter::once(self.attention.estimated_bytes()).chain(
                self.calculators
                    .iter()
                    .map(|value| crate::memory::boxed(value.as_ref(), Graph::estimated_bytes)),
            ),
        )
    }

    fn calculate(
        &self,
        index: usize,
        input: &Tensor,
        rows: usize,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let output = self.calculators[index].run_with(input, budget)?;
        if output.rows() != rows {
            return Err(invalid("transformer child changed the row count"));
        }
        Ok(output)
    }

    fn feed_forward(&self, input: &Tensor, rows: usize, budget: &mut Budget) -> Result<Tensor> {
        let first = self.calculate(0, input, rows, budget)?;
        let activated = self.calculate(4, &first, rows, budget)?;
        self.calculate(1, &activated, rows, budget)
    }

    pub(super) fn run(&self, input: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        let rows = input.rows();
        if self.pre_normalize {
            let normalized = self.calculate(2, input, rows, budget)?;
            let attention = self
                .attention
                .run(&normalized, &normalized, &normalized, budget)?;
            let residual = add(&attention, input, budget)?;
            let normalized = self.calculate(3, &residual, rows, budget)?;
            let output = self.feed_forward(&normalized, rows, budget)?;
            add(&residual, &output, budget)
        } else {
            let attention = self.attention.run(input, input, input, budget)?;
            let residual = add(&attention, input, budget)?;
            let normalized = self.calculate(2, &residual, rows, budget)?;
            let output = self.feed_forward(&normalized, rows, budget)?;
            let residual = add(&normalized, &output, budget)?;
            self.calculate(3, &residual, rows, budget)
        }
    }
}

fn calculator(node: &Node<'_>) -> Result<Box<Graph>> {
    if !matches!(
        node.tag.as_str(),
        "modm0"
            | "modl0"
            | "modl1"
            | "modl2"
            | "modl3"
            | "modl4"
            | "modl5"
            | "modl6"
            | "modl8"
            | "modl9"
            | "moda0"
            | "moda1"
            | "moda2"
            | "moda3"
            | "moda5"
            | "moda7"
    ) {
        return Err(invalid("unsupported transformer calculator"));
    }
    Ok(Box::new(Graph::compile(node)?))
}
