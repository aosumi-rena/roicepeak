use super::{Conditional, Gated};
use crate::{
    Result,
    graph::{
        Budget, add,
        streaming::{Packet, Rows, Stream},
    },
    invalid,
    tensor::Tensor,
};

pub(crate) struct GatedStream<'a> {
    layer: &'a Gated,
    main: Box<Stream<'a>>,
    projection: Option<Box<Stream<'a>>>,
    main_rows: Rows,
    condition_rows: Rows,
}

impl<'a> GatedStream<'a> {
    pub(crate) fn conditioned(&self) -> bool {
        self.layer.condition != 0
    }

    pub(crate) fn new(layer: &'a Gated) -> Result<Self> {
        Ok(Self {
            layer,
            main: Box::new(Stream::new(&layer.main)?),
            projection: layer
                .projection
                .as_deref()
                .map(Stream::new)
                .transpose()?
                .map(Box::new),
            main_rows: Rows::default(),
            condition_rows: Rows::default(),
        })
    }

    pub(crate) fn feed(
        &mut self,
        input: Option<&Tensor>,
        condition: Option<&Tensor>,
        end: bool,
        condition_end: bool,
        budget: &mut Budget,
    ) -> Result<Packet> {
        if input.is_some_and(|input| input.cols() != self.layer.inputs) {
            return Err(invalid("gated input dimensions"));
        }
        if self.projection.is_some()
            && condition.is_some_and(|condition| condition.cols() != self.layer.condition)
        {
            return Err(invalid("gated condition dimensions"));
        }
        let main = self.main.feed(input, None, end, false, budget)?;
        self.main_rows.push(main.data.as_ref(), main.end, budget)?;
        let projected = if let Some(projection) = &mut self.projection {
            let packet = projection.feed(condition, None, condition_end, false, budget)?;
            self.condition_rows
                .push(packet.data.as_ref(), packet.end, budget)?;
            join(&mut self.main_rows, &mut self.condition_rows, budget)?
        } else {
            let rows = self.main_rows.rows();
            let data = self.main_rows.take(rows, budget)?;
            Packet {
                data,
                end: self.main_rows.end(),
            }
        };
        let data = projected
            .data
            .as_ref()
            .map(|projection| self.layer.activate(projection, budget))
            .transpose()?;
        let data = if projected.end && data.is_none() {
            Some(Tensor::zeros(0, self.layer.outputs)?)
        } else {
            data
        };
        Ok(Packet {
            data,
            end: projected.end,
        })
    }
}

struct Stage<'a> {
    gate: Stream<'a>,
    skip: Stream<'a>,
    residual: Stream<'a>,
    gated_rows: Rows,
    residual_rows: Rows,
    skip_rows: Rows,
}

impl Stage<'_> {
    fn feed(
        &mut self,
        input: Option<&Tensor>,
        condition: Option<&Tensor>,
        end: bool,
        condition_end: bool,
        budget: &mut Budget,
    ) -> Result<Packet> {
        let gated = self
            .gate
            .feed(input, condition, end, condition_end, budget)?;
        let residual = self.residual.feed(input, None, end, false, budget)?;
        self.gated_rows
            .push(gated.data.as_ref(), gated.end, budget)?;
        self.residual_rows
            .push(residual.data.as_ref(), residual.end, budget)?;
        let skipped = self
            .skip
            .feed(gated.data.as_ref(), None, gated.end, false, budget)?;
        self.skip_rows
            .push(skipped.data.as_ref(), skipped.end, budget)?;
        join(&mut self.gated_rows, &mut self.residual_rows, budget)
    }
}

pub(crate) struct ConditionalStream<'a> {
    layer: &'a Conditional,
    projection: Option<Box<Stream<'a>>>,
    base_rows: Rows,
    stages: Vec<Stage<'a>>,
}

impl<'a> ConditionalStream<'a> {
    pub(crate) fn conditioned(&self) -> bool {
        self.layer.condition != 0
    }

    pub(crate) fn new(layer: &'a Conditional) -> Result<Self> {
        let stages = layer
            .gates
            .iter()
            .zip(&layer.skips)
            .zip(&layer.residuals)
            .map(|((gate, skip), residual)| {
                Ok(Stage {
                    gate: Stream::new(gate)?,
                    skip: Stream::new(skip)?,
                    residual: Stream::new(residual)?,
                    gated_rows: Rows::default(),
                    residual_rows: Rows::default(),
                    skip_rows: Rows::default(),
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            layer,
            projection: layer
                .projection
                .as_deref()
                .map(Stream::new)
                .transpose()?
                .map(Box::new),
            base_rows: Rows::default(),
            stages,
        })
    }

    pub(crate) fn feed(
        &mut self,
        input: Option<&Tensor>,
        condition: Option<&Tensor>,
        end: bool,
        condition_end: bool,
        budget: &mut Budget,
    ) -> Result<Packet> {
        if input.is_some_and(|input| input.cols() != self.layer.inputs) {
            return Err(invalid("conditional input dimensions"));
        }
        if self.projection.is_some()
            && condition.is_some_and(|condition| condition.cols() != self.layer.condition)
        {
            return Err(invalid("conditional condition dimensions"));
        }
        let base = if let Some(projection) = &mut self.projection {
            projection.feed(condition, None, condition_end, false, budget)?
        } else {
            let data = input
                .map(|input| {
                    budget.spend(0, input.rows().saturating_mul(self.layer.outputs))?;
                    Tensor::zeros(input.rows(), self.layer.outputs)
                })
                .transpose()?;
            Packet { data, end }
        };
        self.base_rows.push(base.data.as_ref(), base.end, budget)?;
        if let Some(input) = input {
            budget.spend(0, input.values().len())?;
        }
        let mut state = Packet {
            data: input.cloned(),
            end,
        };
        for stage in &mut self.stages {
            state = stage.feed(
                state.data.as_ref(),
                condition,
                state.end,
                condition_end,
                budget,
            )?;
        }
        let rows = self
            .stages
            .iter()
            .fold(self.base_rows.rows(), |rows, stage| {
                rows.min(stage.skip_rows.rows())
            });
        let mut data = self.base_rows.take(rows, budget)?;
        for stage in &mut self.stages {
            let skip = stage.skip_rows.take(rows, budget)?;
            if let (Some(output), Some(skip)) = (data.as_ref(), skip.as_ref()) {
                data = Some(add(output, skip, budget)?);
            }
        }
        let branches_ended = self.base_rows.input_ended()
            && self
                .stages
                .iter()
                .all(|stage| stage.skip_rows.input_ended());
        if branches_ended
            && (self.base_rows.rows() != 0
                || self.stages.iter().any(|stage| stage.skip_rows.rows() != 0))
        {
            return Err(invalid("conditional stream branch row counts differ"));
        }
        let end = state.end
            && self.base_rows.end()
            && self.stages.iter().all(|stage| stage.skip_rows.end());
        let data = if end && data.is_none() {
            Some(Tensor::zeros(0, self.layer.outputs)?)
        } else {
            data
        };
        Ok(Packet { data, end })
    }
}

fn join(left: &mut Rows, right: &mut Rows, budget: &mut Budget) -> Result<Packet> {
    let rows = left.rows().min(right.rows());
    let a = left.take(rows, budget)?;
    let b = right.take(rows, budget)?;
    let data = match (a, b) {
        (Some(a), Some(b)) => Some(add(&a, &b, budget)?),
        _ => None,
    };
    if left.input_ended() && right.input_ended() && left.rows() != right.rows() {
        return Err(invalid("stream branch row counts differ"));
    }
    Ok(Packet {
        data,
        end: left.end() && right.end(),
    })
}
