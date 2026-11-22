use crate::{
    Result,
    dnni::Node,
    graph::Budget,
    invalid,
    tensor::{Tensor, checked_elements},
};

const CHANNELS: usize = 72;
const PASSTHROUGH: usize = 2;
const FILTERED: usize = CHANNELS - PASSTHROUGH;
type Coefficients = [f32; FILTERED];

pub(super) struct SpectralFilter {
    taps: Vec<Coefficients>,
    gain: Coefficients,
    bias: Coefficients,
}

impl SpectralFilter {
    pub(super) fn compile(node: &Node<'_>) -> Result<Self> {
        let [coefficients, gain, bias] = node.children.as_slice() else {
            return Err(invalid("invalid legacy spectral filter"));
        };
        if node.tag != "_mspfv0"
            || !node.payload.is_empty()
            || coefficients.tag != "cmpg1"
            || !coefficients.payload.is_empty()
            || coefficients.children.len() % 2 == 0
            || coefficients.children.len() > 4096
        {
            return Err(invalid("invalid legacy spectral filter"));
        }
        Ok(Self {
            taps: coefficients
                .children
                .iter()
                .map(vector)
                .collect::<Result<_>>()?,
            gain: vector(gain)?,
            bias: vector(bias)?,
        })
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        crate::memory::values(&self.taps)
    }

    pub(super) fn stream(&self) -> Stream<'_> {
        Stream {
            filter: self,
            input: Vec::new(),
            origin: 0,
            produced: 0,
        }
    }
}

fn vector(node: &Node<'_>) -> Result<Coefficients> {
    if node.tag != "prim1" {
        return Err(invalid("invalid legacy spectral filter vector"));
    }
    node.tensor()?
        .values()
        .try_into()
        .map_err(|_| invalid("invalid legacy spectral filter width"))
}

pub(super) struct Stream<'a> {
    filter: &'a SpectralFilter,
    input: Vec<f32>,
    origin: usize,
    produced: usize,
}

impl Stream<'_> {
    pub(super) fn feed(
        &mut self,
        input: Option<&Tensor>,
        end: bool,
        budget: &mut Budget,
    ) -> Result<Option<Tensor>> {
        if let Some(input) = input {
            if input.cols() != CHANNELS {
                return Err(invalid("invalid legacy spectral filter input"));
            }
            let received = self.origin + self.input.len() / CHANNELS + input.rows();
            checked_elements(received, CHANNELS)?;
            budget.spend(0, input.values().len())?;
            self.input.extend_from_slice(input.values());
        }
        let received = self.origin + self.input.len() / CHANNELS;
        let radius = self.filter.taps.len() / 2;
        let ready = if end {
            received
        } else {
            received.saturating_sub(radius)
        };
        let frames = ready - self.produced;
        if frames == 0 {
            return Ok(None);
        }
        budget.spend(
            frames.saturating_mul(FILTERED.saturating_mul(self.filter.taps.len() + 1)),
            checked_elements(frames, CHANNELS)?,
        )?;
        let mut output = Tensor::zeros(frames, CHANNELS)?;
        for (index, frame) in (self.produced..ready).enumerate() {
            let out = output.row_mut(index);
            let begin = (frame - self.origin) * CHANNELS;
            out[..PASSTHROUGH].copy_from_slice(&self.input[begin..begin + PASSTHROUGH]);
            for (tap, weights) in self.filter.taps.iter().enumerate() {
                let position = (frame + tap).saturating_sub(radius).min(received - 1);
                let start = (position - self.origin) * CHANNELS + PASSTHROUGH;
                let row = &self.input[start..start + FILTERED];
                for ((sum, value), weight) in out[PASSTHROUGH..].iter_mut().zip(row).zip(weights) {
                    *sum += value * weight;
                }
            }
            for ((value, gain), bias) in out[PASSTHROUGH..]
                .iter_mut()
                .zip(&self.filter.gain)
                .zip(&self.filter.bias)
            {
                *value = *value * gain + bias;
            }
        }
        self.produced = ready;
        let retain = ready.saturating_sub(radius);
        let discard = (retain - self.origin) * CHANNELS;
        drop(self.input.drain(..discard));
        self.origin = retain;
        Ok(Some(output))
    }
}
