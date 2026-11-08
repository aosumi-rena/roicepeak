use super::super::dsp::{HOP, LtvStream};
use crate::{
    Result,
    graph::{
        Budget,
        streaming::{Packet, Rows},
    },
    invalid,
    tensor::Tensor,
};

pub(super) fn pair(
    left: &mut Rows,
    right: &mut Rows,
    ratio: usize,
    budget: &mut Budget,
) -> Result<(Option<Tensor>, Option<Tensor>, bool)> {
    let rows = left.rows().min(right.rows() / ratio);
    let a = left.take(rows, budget)?;
    let b = right.take(rows * ratio, budget)?;
    if left.input_ended() && right.input_ended() && (left.rows() != 0 || right.rows() != 0) {
        return Err(invalid("vocoder stream branch lengths differ"));
    }
    Ok((a, b, left.end() && right.end()))
}

pub(super) fn ltv(
    stream: &mut LtvStream<'_>,
    parameters: &mut Rows,
    signal: &mut Rows,
    budget: &mut Budget,
) -> Result<Packet> {
    let columns = parameters.columns();
    let (parameters, signal, end) = pair(parameters, signal, HOP, budget)?;
    if parameters.is_none() && !end {
        return Ok(Packet {
            data: None,
            end: false,
        });
    }
    let parameters = match parameters {
        Some(parameters) => parameters,
        None => Tensor::zeros(
            0,
            columns.ok_or_else(|| invalid("LTV parameter channels unavailable"))?,
        )?,
    };
    let signal = signal.unwrap_or(Tensor::zeros(0, 1)?);
    Ok(Packet {
        data: Some(stream.feed(&parameters, &signal, end, budget)?),
        end,
    })
}
