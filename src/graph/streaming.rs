use super::{Budget, Graph, conditional, convolution};
use crate::{
    Result, invalid,
    tensor::{Tensor, checked_elements},
};

pub(crate) struct Packet {
    pub data: Option<Tensor>,
    pub end: bool,
}

#[derive(Default)]
pub(crate) struct Rows {
    values: Vec<f32>,
    columns: Option<usize>,
    head: usize,
    ended: bool,
}

impl Rows {
    pub(crate) fn push(
        &mut self,
        input: Option<&Tensor>,
        end: bool,
        budget: &mut Budget,
    ) -> Result<()> {
        if let Some(input) = input {
            if self.ended && input.rows() != 0 {
                return Err(invalid("stream input arrived after its end"));
            }
            if self.columns.is_some_and(|columns| columns != input.cols()) {
                return Err(invalid("stream channel count changed"));
            }
            self.columns = Some(input.cols());
            let rows = self
                .rows()
                .checked_add(input.rows())
                .ok_or_else(|| invalid("stream queue length overflow"))?;
            checked_elements(rows, input.cols())?;
            budget.spend(0, input.values().len())?;
            self.compact();
            self.values.extend_from_slice(input.values());
        }
        self.ended |= end;
        Ok(())
    }

    pub(crate) fn rows(&self) -> usize {
        self.columns
            .map_or(0, |columns| (self.values.len() - self.head) / columns)
    }

    pub(crate) fn input_ended(&self) -> bool {
        self.ended
    }

    pub(crate) fn columns(&self) -> Option<usize> {
        self.columns
    }

    pub(crate) fn end(&self) -> bool {
        self.ended && self.rows() == 0
    }

    pub(crate) fn take(&mut self, rows: usize, budget: &mut Budget) -> Result<Option<Tensor>> {
        if rows == 0 {
            return Ok(None);
        }
        if rows > self.rows() {
            return Err(invalid("stream queue has insufficient rows"));
        }
        let columns = self
            .columns
            .ok_or_else(|| invalid("stream channels are unavailable"))?;
        let count = checked_elements(rows, columns)?;
        budget.spend(0, count)?;
        let values = self.values[self.head..self.head + count].to_vec();
        self.head += count;
        self.compact();
        Ok(Some(Tensor::new(rows, columns, values)?))
    }

    fn compact(&mut self) {
        if self.head != 0 && self.head >= self.values.len() / 2 {
            self.values.drain(..self.head);
            self.head = 0;
        }
    }
}

enum State<'a> {
    Sequential(Vec<Stream<'a>>),
    Convolution(convolution::streaming::Stream<'a>),
    Gated(conditional::streaming::GatedStream<'a>),
    Conditional(conditional::streaming::ConditionalStream<'a>),
    Recurrent(&'a Graph, Vec<f32>),
    Pointwise(&'a Graph),
}

pub(crate) struct Stream<'a> {
    state: State<'a>,
    ended: bool,
    conditioned: bool,
}

impl<'a> Stream<'a> {
    pub(crate) fn new(graph: &'a Graph) -> Result<Self> {
        let state = match graph {
            Graph::Sequential(nodes) => {
                State::Sequential(nodes.iter().map(Self::new).collect::<Result<_>>()?)
            }
            Graph::Convolution(layer) => State::Convolution(layer.stream()?),
            Graph::Gated(layer) => State::Gated(conditional::streaming::GatedStream::new(layer)?),
            Graph::Conditional(layer) => {
                State::Conditional(conditional::streaming::ConditionalStream::new(layer)?)
            }
            Graph::Gru(_) => State::Recurrent(graph, vec![0.0; graph.recurrent_state_len()?]),
            Graph::Affine(_)
            | Graph::Activation(_)
            | Graph::GeneratedActivation(_)
            | Graph::Silu(_)
            | Graph::ChannelAffine(_, _)
            | Graph::Embedding(_) => State::Pointwise(graph),
            Graph::Normalization(layer) if layer.kind() == "layer_norm" => State::Pointwise(graph),
            _ => return Err(invalid("graph operator requires a complete sequence")),
        };
        let conditioned = match &state {
            State::Sequential(nodes) => nodes.iter().any(|node| node.conditioned),
            State::Gated(layer) => layer.conditioned(),
            State::Conditional(layer) => layer.conditioned(),
            _ => false,
        };
        Ok(Self {
            state,
            ended: false,
            conditioned,
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
        if self.ended {
            if input.is_some_and(|input| input.rows() != 0)
                || (self.conditioned && condition.is_some_and(|condition| condition.rows() != 0))
            {
                return Err(invalid("graph input arrived after its end"));
            }
            return Ok(Packet {
                data: None,
                end: true,
            });
        }
        let packet = match &mut self.state {
            State::Sequential(nodes) => {
                if let Some(input) = input {
                    budget.spend(0, input.values().len())?;
                }
                let mut packet = Packet {
                    data: input.cloned(),
                    end,
                };
                for node in nodes {
                    packet = node.feed(
                        packet.data.as_ref(),
                        condition,
                        packet.end,
                        condition_end,
                        budget,
                    )?;
                }
                packet
            }
            State::Convolution(layer) => layer.feed(input, end, budget)?,
            State::Gated(layer) => layer.feed(input, condition, end, condition_end, budget)?,
            State::Conditional(layer) => {
                layer.feed(input, condition, end, condition_end, budget)?
            }
            State::Recurrent(graph, hidden) => Packet {
                data: input
                    .map(|input| graph.run_recurrent(input, hidden, budget))
                    .transpose()?,
                end,
            },
            State::Pointwise(graph) => Packet {
                data: input
                    .map(|input| graph.run_with(input, budget))
                    .transpose()?,
                end,
            },
        };
        if let Some(data) = &packet.data {
            data.ensure_finite()?;
        }
        self.ended = packet.end;
        Ok(packet)
    }
}
