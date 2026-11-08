use super::lf_math::Mode;
use crate::{Result, graph::Budget, tensor::Tensor};

mod streaming;
pub(super) use streaming::Stream;

pub(super) fn excitation(
    physical: &Tensor,
    shape: &Tensor,
    gain: f32,
    budget: &mut Budget,
) -> Result<Tensor> {
    with_mode(physical, shape, gain, budget, Mode::current())
}

pub(super) fn with_mode(
    physical: &Tensor,
    shape: &Tensor,
    gain: f32,
    budget: &mut Budget,
    mode: Mode,
) -> Result<Tensor> {
    with_mode_and_voicing(physical, shape, gain, false, budget, mode)
}

fn with_mode_and_voicing(
    physical: &Tensor,
    shape: &Tensor,
    gain: f32,
    force_voiced: bool,
    budget: &mut Budget,
    mode: Mode,
) -> Result<Tensor> {
    Stream::new(gain, force_voiced, mode)?.feed(physical, shape, true, budget)
}
