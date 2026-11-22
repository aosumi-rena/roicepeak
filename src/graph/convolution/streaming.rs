use super::Convolution;
use crate::{
    Result,
    graph::{Budget, profiling, streaming::Packet},
    invalid,
    tensor::{Tensor, checked_elements},
};

#[derive(Debug)]
pub(crate) struct Stream<'a> {
    model: &'a Convolution,
    input: Vec<f32>,
    origin: usize,
    received: usize,
    next_output: usize,
    ended: bool,
}

impl<'a> Stream<'a> {
    pub(super) fn new(model: &'a Convolution) -> Result<Self> {
        model.span()?;
        if model.channels().0 == 0 {
            return Err(invalid("convolution stream requires input channels"));
        }
        Ok(Self {
            model,
            input: Vec::new(),
            origin: 0,
            received: 0,
            next_output: 0,
            ended: false,
        })
    }

    pub(crate) fn feed(
        &mut self,
        input: Option<&Tensor>,
        end: bool,
        budget: &mut Budget,
    ) -> Result<Packet> {
        if self.ended {
            if input.is_some_and(|value| value.rows() != 0) {
                return Err(invalid("convolution input after stream end"));
            }
            return Ok(Packet {
                data: None,
                end: true,
            });
        }
        let (channels, _) = self.model.channels();
        if let Some(input) = input {
            if input.cols() != channels {
                return Err(invalid("convolution stream input dimensions"));
            }
            input.ensure_finite()?;
            let received = self
                .received
                .checked_add(input.rows())
                .ok_or_else(|| invalid("convolution stream input overflow"))?;
            checked_elements(received, channels)?;
            budget.spend(0, input.values().len())?;
            self.input.extend_from_slice(input.values());
            self.received = received;
        }
        let ready = if end {
            self.model.output_rows(self.received)?
        } else {
            let available = self
                .received
                .checked_add(self.model.padding)
                .ok_or_else(|| invalid("convolution stream padding overflow"))?;
            let span = self.model.span()?;
            if available < span {
                0
            } else {
                (available - span) / self.model.stride + 1
            }
        };
        let frames = ready
            .checked_sub(self.next_output)
            .ok_or_else(|| invalid("convolution stream output cursor"))?;
        let data = if frames == 0 {
            if end {
                Some(Tensor::zeros(0, self.model.channels().1)?)
            } else {
                None
            }
        } else {
            let timing = profiling::enter(
                "streaming_convolution",
                input.map_or(0, Tensor::rows),
                channels,
                budget.multiply_adds,
            );
            let output = self
                .model
                .run_range(
                    &self.input,
                    self.origin,
                    self.received,
                    self.next_output,
                    frames,
                    budget,
                )
                .and_then(|output| {
                    output.ensure_finite()?;
                    Ok(output)
                });
            if let Some(timing) = timing {
                timing.finish(
                    output
                        .as_ref()
                        .ok()
                        .map(|value| (value.rows(), value.cols())),
                    budget.multiply_adds,
                );
            }
            let output = output?;
            Some(output)
        };
        self.next_output = ready;
        self.ended = end;
        let retain = if end {
            self.received
        } else {
            self.next_output
                .checked_mul(self.model.stride)
                .ok_or_else(|| invalid("convolution stream input cursor overflow"))?
                .saturating_sub(self.model.padding)
                .min(self.received)
        };
        let discard = retain
            .checked_sub(self.origin)
            .and_then(|rows| rows.checked_mul(channels))
            .filter(|words| *words <= self.input.len())
            .ok_or_else(|| invalid("convolution stream retained input cursor"))?;
        drop(self.input.drain(..discard));
        self.origin = retain;
        Ok(Packet { data, end })
    }
}
