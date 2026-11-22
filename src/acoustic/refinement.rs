use crate::{
    Result,
    dnni::Node,
    graph::{
        Budget, Graph,
        streaming::{Packet, Rows, Stream},
    },
    invalid,
    sequence::concatenate,
    tensor::Tensor,
};

pub(super) struct Refinement {
    project: Graph,
    condition: Graph,
    head: Graph,
}

impl Refinement {
    pub(super) fn compile(
        project: &Node<'_>,
        condition: &Node<'_>,
        head: &Node<'_>,
    ) -> Result<Self> {
        if project.tag != "modl0" || condition.tag != "_ncwnv0" {
            return Err(invalid("unsupported legacy acoustic refinement"));
        }
        Ok(Self {
            project: Graph::compile(project)?,
            condition: Graph::compile(condition)?,
            head: Graph::compile(head)?,
        })
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.project.estimated_bytes(),
            self.condition.estimated_bytes(),
            self.head.estimated_bytes(),
        ])
    }

    pub(super) fn stream(&self) -> Result<RefinementStream<'_>> {
        Ok(RefinementStream {
            project: &self.project,
            condition: self.condition.stream()?,
            head: self.head.stream()?,
            base: Rows::default(),
        })
    }
}

pub(super) struct RefinementStream<'a> {
    project: &'a Graph,
    condition: Stream<'a>,
    head: Stream<'a>,
    base: Rows,
}

impl RefinementStream<'_> {
    pub(super) fn feed(
        &mut self,
        context: &Tensor,
        hidden: &Tensor,
        base: &Tensor,
        end: bool,
        budget: &mut Budget,
    ) -> Result<Packet> {
        let input = concatenate(&[context, hidden], budget)?;
        let condition = self.project.run_with(&input, budget)?;
        self.base.push(Some(base), end, budget)?;
        let filtered = self
            .condition
            .feed(Some(base), Some(&condition), end, end, budget)?;
        let mut result =
            self.head
                .feed(filtered.data.as_ref(), None, filtered.end, false, budget)?;
        if let Some(refined) = result.data.as_mut()
            && let Some(base) = self.base.take(refined.rows(), budget)?
        {
            if refined.cols() != base.cols() {
                return Err(invalid("legacy acoustic refinement channels differ"));
            }
            for (result, base) in refined.values_mut().iter_mut().zip(base.values()) {
                *result = *base + *result;
            }
        }
        if result.end != self.base.end() {
            return Err(invalid("legacy acoustic refinement length differs"));
        }
        Ok(result)
    }
}
