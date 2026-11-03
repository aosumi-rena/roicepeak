use super::{Budget, Graph, add, children, payload};
use crate::{
    Result,
    activation::sigmoid,
    dnni::Node,
    invalid,
    tensor::{Tensor, u32_at},
};

#[derive(Debug)]
pub struct Gated {
    inputs: usize,
    outputs: usize,
    condition: usize,
    main: Box<Graph>,
    projection: Option<Box<Graph>>,
}
impl Gated {
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        m::sum([
            m::boxed(self.main.as_ref(), Graph::estimated_bytes),
            m::optional(&self.projection, |value| {
                m::boxed(value.as_ref(), Graph::estimated_bytes)
            }),
        ])
    }

    pub(super) fn from_node(n: &Node<'_>) -> Result<Self> {
        payload(n, 12)?;
        let inputs = u32_at(n.payload, 0)? as usize;
        let outputs = u32_at(n.payload, 4)? as usize;
        let condition = u32_at(n.payload, 8)? as usize;
        if inputs == 0 || outputs == 0 {
            return Err(invalid("zero gated channels"));
        }
        children(n, &[if condition == 0 { 1 } else { 2 }])?;
        let main = Box::new(convolution(&n.children[0], inputs, 2 * outputs)?);
        let projection = if condition == 0 {
            None
        } else {
            Some(Box::new(convolution(
                &n.children[1],
                condition,
                2 * outputs,
            )?))
        };
        Ok(Self {
            inputs,
            outputs,
            condition,
            main,
            projection,
        })
    }
    pub(super) fn run(
        &self,
        x: &Tensor,
        condition: Option<&Tensor>,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if x.cols() != self.inputs {
            return Err(invalid("gated input dimensions"));
        }
        let mut projection = self.main.run_with(x, budget)?;
        if let Some(layer) = &self.projection {
            let c = condition.ok_or_else(|| invalid("gated condition required"))?;
            if c.cols() != self.condition {
                return Err(invalid("gated condition dimensions"));
            }
            projection = add(&projection, &layer.run_with(c, budget)?, budget)?;
        }
        budget.spend(
            projection
                .rows()
                .saturating_mul(self.outputs)
                .saturating_mul(32),
            projection.rows().saturating_mul(self.outputs),
        )?;
        let mut y = Tensor::zeros(projection.rows(), self.outputs)?;
        for t in 0..y.rows() {
            for c in 0..self.outputs {
                y.row_mut(t)[c] =
                    projection.row(t)[c].tanh() * sigmoid(projection.row(t)[self.outputs + c]);
            }
        }
        Ok(y)
    }
}

#[derive(Debug)]
pub struct Conditional {
    inputs: usize,
    outputs: usize,
    condition: usize,
    gates: Vec<Graph>,
    skips: Vec<Graph>,
    residuals: Vec<Graph>,
    projection: Option<Box<Graph>>,
}
impl Conditional {
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        m::sum([
            m::nested(&self.gates, Graph::estimated_bytes),
            m::nested(&self.skips, Graph::estimated_bytes),
            m::nested(&self.residuals, Graph::estimated_bytes),
            m::optional(&self.projection, |value| {
                m::boxed(value.as_ref(), Graph::estimated_bytes)
            }),
        ])
    }

    pub(super) fn from_node(n: &Node<'_>) -> Result<Self> {
        payload(n, 12)?;
        let inputs = u32_at(n.payload, 0)? as usize;
        let outputs = u32_at(n.payload, 4)? as usize;
        let condition = u32_at(n.payload, 8)? as usize;
        if inputs == 0 || outputs == 0 {
            return Err(invalid("zero conditional channels"));
        }
        children(n, &[if condition == 0 { 3 } else { 4 }])?;
        let lists = &n.children[..3];
        if lists
            .iter()
            .any(|c| c.tag != "cmpg1" || !c.payload.is_empty())
            || lists[0].children.is_empty()
            || lists
                .iter()
                .any(|c| c.children.len() != lists[0].children.len())
        {
            return Err(invalid("conditional branch list configuration"));
        }
        let mut gates = Vec::new();
        let mut skips = Vec::new();
        let mut residuals = Vec::new();
        let mut channels = inputs;
        for i in 0..lists[0].children.len() {
            let n = &lists[0].children[i];
            if n.tag != "_gnc1v0" {
                return Err(invalid("conditional branch must be gated"));
            }
            let gate = Gated::from_node(n)?;
            if gate.inputs != channels || gate.condition != condition {
                return Err(invalid("conditional gate channels"));
            }
            skips.push(convolution(&lists[1].children[i], gate.outputs, outputs)?);
            residuals.push(convolution(&lists[2].children[i], channels, gate.outputs)?);
            channels = gate.outputs;
            gates.push(Graph::Gated(gate));
        }
        let projection = if condition == 0 {
            None
        } else {
            Some(Box::new(convolution(&n.children[3], condition, outputs)?))
        };
        Ok(Self {
            inputs,
            outputs,
            condition,
            gates,
            skips,
            residuals,
            projection,
        })
    }
    pub(super) fn run(
        &self,
        x: &Tensor,
        condition: Option<&Tensor>,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if x.cols() != self.inputs {
            return Err(invalid("conditional input dimensions"));
        }
        budget.spend(0, x.values().len())?;
        let mut state = x.clone();
        let mut output = if let Some(p) = &self.projection {
            let c = condition.ok_or_else(|| invalid("conditional input required"))?;
            if c.cols() != self.condition {
                return Err(invalid("conditional condition dimensions"));
            }
            p.run_with(c, budget)?
        } else {
            budget.spend(0, x.rows().saturating_mul(self.outputs))?;
            Tensor::zeros(x.rows(), self.outputs)?
        };
        for ((gate, skip), residual) in self.gates.iter().zip(&self.skips).zip(&self.residuals) {
            let gated = gate.run_optional(&state, condition, budget)?;
            state = add(&gated, &residual.run_with(&state, budget)?, budget)?;
            output = add(&output, &skip.run_with(&gated, budget)?, budget)?;
        }
        Ok(output)
    }
}
fn convolution(n: &Node<'_>, inputs: usize, outputs: usize) -> Result<Graph> {
    if n.tag != "modl1" {
        return Err(invalid(
            "conditional projection must be ordinary convolution",
        ));
    }
    let graph = Graph::compile(n)?;
    let Graph::Convolution(c) = &graph else {
        unreachable!()
    };
    if c.channels() != (inputs, outputs) {
        return Err(invalid("conditional convolution dimensions"));
    }
    Ok(graph)
}
