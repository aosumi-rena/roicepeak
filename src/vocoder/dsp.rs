#[path = "dsp/ltv_stream.rs"]
mod ltv_stream;
#[path = "dsp/noise_stream.rs"]
mod noise_stream;
#[path = "dsp/upsample_stream.rs"]
mod upsample_stream;
use crate::{Result, graph::Budget, tensor::Tensor};
pub(in crate::vocoder) use ltv_stream::Stream as LtvStream;
pub(in crate::vocoder) use noise_stream::Noise;
pub(in crate::vocoder) use upsample_stream::Upsampler;
pub const HOP: usize = 240;
const BANDS: usize = 8;

pub fn postfilter_noise(rows: usize, budget: &mut Budget) -> Result<Tensor> {
    postfilter_noise_mode(rows, super::postfilter_log::fused_available(), budget)
}
pub(super) fn postfilter_noise_mode(
    rows: usize,
    fused: bool,
    budget: &mut Budget,
) -> Result<Tensor> {
    Noise::new(fused).generate(rows, budget)
}
