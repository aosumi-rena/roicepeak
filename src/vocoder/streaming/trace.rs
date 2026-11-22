use crate::{
    Result,
    graph::Budget,
    invalid,
    tensor::{Tensor, checked_elements},
};
use std::collections::BTreeMap;

pub(super) struct Trace {
    intermediates: bool,
    stages: BTreeMap<&'static str, (usize, Vec<f32>)>,
}

impl Trace {
    pub(super) fn new(intermediates: bool) -> Self {
        Self {
            intermediates,
            stages: BTreeMap::new(),
        }
    }

    pub(super) fn push(
        &mut self,
        name: &'static str,
        input: Option<&Tensor>,
        budget: &mut Budget,
    ) -> Result<()> {
        if (self.intermediates || name == "pcm")
            && let Some(input) = input
        {
            let (columns, values) = self
                .stages
                .entry(name)
                .or_insert_with(|| (input.cols(), Vec::new()));
            if *columns != input.cols() {
                return Err(invalid("vocoder trace channels changed"));
            }
            checked_elements(values.len() / *columns + input.rows(), *columns)?;
            budget.spend(0, input.values().len())?;
            values.extend_from_slice(input.values());
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Result<BTreeMap<&'static str, Tensor>> {
        self.stages
            .into_iter()
            .map(|(name, (columns, values))| {
                Ok((name, Tensor::new(values.len() / columns, columns, values)?))
            })
            .collect()
    }
}
