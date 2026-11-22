use super::Sampler;
use crate::{
    Result,
    graph::{
        Budget,
        streaming::{Packet, Rows, Stream as GraphStream},
    },
    invalid,
    sequence::concatenate,
    tensor::Tensor,
};

pub(crate) struct Stream<'a> {
    model: &'a Sampler,
    input: GraphStream<'a>,
    conditional: GraphStream<'a>,
    output: GraphStream<'a>,
    context: Rows,
    noise: Rows,
    shape: f32,
    offset: f32,
    feature_columns: Option<usize>,
    feature_rows: usize,
    condition_rows: usize,
    features_ended: bool,
    ended: bool,
}

impl<'a> Stream<'a> {
    pub(super) fn new(model: &'a Sampler, shape: f32, offset: f32) -> Result<Self> {
        if !shape.is_finite() || !offset.is_finite() {
            return Err(invalid("sampler stream shape or offset"));
        }
        Ok(Self {
            model,
            input: model.input.stream()?,
            conditional: model.conditional.stream()?,
            output: model.output.stream()?,
            context: Rows::default(),
            noise: Rows::default(),
            shape,
            offset,
            feature_columns: None,
            feature_rows: 0,
            condition_rows: 0,
            features_ended: false,
            ended: false,
        })
    }

    pub(crate) fn feed(
        &mut self,
        features: Option<&Tensor>,
        features_end: bool,
        context: Option<&Tensor>,
        context_end: bool,
        noise: Option<&Tensor>,
        noise_end: bool,
        budget: &mut Budget,
    ) -> Result<Packet> {
        if self.ended {
            if [features, context, noise]
                .iter()
                .any(|input| input.is_some_and(|input| input.rows() != 0))
            {
                return Err(invalid("sampler input arrived after its end"));
            }
            return Ok(Packet {
                data: None,
                end: true,
            });
        }
        if noise.is_some_and(|noise| noise.cols() != self.model.channels) {
            return Err(invalid("sampler noise dimensions"));
        }
        if let Some(features) = features {
            if self.features_ended && features.rows() != 0 {
                return Err(invalid("sampler features arrived after their end"));
            }
            if self
                .feature_columns
                .is_some_and(|columns| columns != features.cols())
            {
                return Err(invalid("sampler feature channel count changed"));
            }
            self.feature_columns = Some(features.cols());
            self.feature_rows = self
                .feature_rows
                .checked_add(features.rows())
                .ok_or_else(|| invalid("sampler feature row count overflow"))?;
        }
        self.features_ended |= features_end;
        self.context.push(context, context_end, budget)?;
        self.noise.push(noise, noise_end, budget)?;
        let rows = self.context.rows().min(self.noise.rows());
        let context = self.context.take(rows, budget)?;
        let noise = self.noise.take(rows, budget)?;
        if self.context.input_ended()
            && self.noise.input_ended()
            && (self.context.rows() != 0 || self.noise.rows() != 0)
        {
            return Err(invalid("sampler context and noise row counts differ"));
        }
        self.condition_rows = self
            .condition_rows
            .checked_add(rows)
            .ok_or_else(|| invalid("sampler condition row count overflow"))?;
        let condition_end = self.context.end() && self.noise.end();
        if self.features_ended && condition_end && self.feature_rows != self.condition_rows {
            return Err(invalid("sampler feature and condition row counts differ"));
        }
        let condition = match (context.as_ref(), noise.as_ref()) {
            (Some(context), Some(noise)) => {
                let shape = self.model.embed(&vec![self.shape; rows], budget)?;
                let offset = self.model.embed(&vec![self.offset; rows], budget)?;
                Some(concatenate(&[context, &shape, &offset, noise], budget)?)
            }
            _ => None,
        };
        let input = self
            .input
            .feed(features, None, self.features_ended, false, budget)?;
        let conditional = self.conditional.feed(
            input.data.as_ref(),
            condition.as_ref(),
            input.end,
            condition_end,
            budget,
        )?;
        let mut output = self.output.feed(
            conditional.data.as_ref(),
            None,
            conditional.end,
            false,
            budget,
        )?;
        output.end &= self.features_ended && condition_end;
        self.ended = output.end;
        Ok(output)
    }
}
