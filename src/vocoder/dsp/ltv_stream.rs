use super::super::{
    effects::{Effects, SpectrumKind},
    ltv_window, postfilter_log, real_fft, response,
};
use super::HOP;
use crate::{
    Result,
    graph::Budget,
    invalid,
    tensor::{Tensor, checked_elements},
};

const SIZE: usize = 1024;
const WINDOW: usize = HOP * 2;
const PADDING: usize = (SIZE - WINDOW) / 2;

pub(in crate::vocoder) struct Stream<'a> {
    spectral: bool,
    effect: Option<(&'a Effects, SpectrumKind, f32, f32)>,
    fused: bool,
    window: [f32; WINDOW],
    previous: [f32; HOP],
    overlap: [f32; SIZE],
    crop: usize,
    frames: usize,
    produced: usize,
    partial: [f32; HOP],
    partial_words: usize,
    ended: bool,
    columns: Option<usize>,
}

impl<'a> Stream<'a> {
    pub(in crate::vocoder) fn new(
        spectral: bool,
        effect: Option<(&'a Effects, SpectrumKind, f32, f32)>,
    ) -> Self {
        Self {
            spectral,
            effect,
            fused: postfilter_log::fused_available(),
            window: ltv_window::window(),
            previous: [0.0; HOP],
            overlap: [0.0; SIZE],
            crop: SIZE / 2,
            frames: 0,
            produced: 0,
            partial: [0.0; HOP],
            partial_words: 0,
            ended: false,
            columns: None,
        }
    }

    pub(in crate::vocoder) fn feed(
        &mut self,
        parameters: &Tensor,
        signal: &Tensor,
        end: bool,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let length = parameters
            .rows()
            .checked_mul(HOP)
            .ok_or_else(|| invalid("LTV frame overflow"))?;
        if signal.cols() != 1
            || signal.rows() != length
            || parameters.cols() == 0
            || parameters.cols() % 2 != 0
            || (self.spectral && parameters.cols() != 2 * (SIZE / 2 + 1))
            || (!self.spectral && parameters.cols() > SIZE)
            || self
                .columns
                .is_some_and(|columns| columns != parameters.cols())
        {
            return Err(invalid("LTV dimensions"));
        }
        if self.ended {
            return Err(invalid("LTV input arrived after its end"));
        }
        self.columns = Some(parameters.cols());
        self.frames = self
            .frames
            .checked_add(parameters.rows())
            .ok_or_else(|| invalid("LTV frame overflow"))?;
        let target = self
            .frames
            .checked_mul(HOP)
            .ok_or_else(|| invalid("LTV sample overflow"))?;
        checked_elements(target, 1)?;
        let steps = parameters
            .rows()
            .checked_add(if end && self.frames != 0 { 3 } else { 0 })
            .ok_or_else(|| invalid("LTV drain overflow"))?;
        budget.spend(steps.saturating_mul(SIZE * 40), steps.saturating_mul(HOP))?;
        let mut output = Vec::new();
        for frame in 0..steps {
            let packed = if frame < parameters.rows() {
                let mut value =
                    response::convert(parameters.row(frame), self.spectral, self.fused)?;
                if let Some((effects, kind, tension, rate)) = self.effect {
                    budget.spend(SIZE * 128, SIZE)?;
                    effects.apply(&mut value, kind, tension, rate)?;
                }
                value
            } else {
                [0.0; SIZE]
            };
            let current = if frame < parameters.rows() {
                &signal.values()[frame * HOP..(frame + 1) * HOP]
            } else {
                &[0.0; HOP]
            };
            let mut buffer = [0.0; SIZE];
            for index in 0..HOP {
                buffer[PADDING + index] =
                    ltv_window::source_product(self.previous[index], self.window[index]);
                buffer[PADDING + HOP + index] =
                    ltv_window::source_product(current[index], self.window[HOP + index]);
            }
            self.previous.copy_from_slice(current);
            real_fft::forward(&mut buffer);
            real_fft::multiply(&mut buffer, &packed);
            real_fft::inverse(&mut buffer);
            ltv_window::overlap(&mut self.overlap, &buffer);
            let skip = self.crop.min(HOP);
            self.crop -= skip;
            let mut first = skip;
            while first < HOP {
                let count = (HOP - first).min(HOP - self.partial_words);
                self.partial[self.partial_words..self.partial_words + count]
                    .copy_from_slice(&self.overlap[first..first + count]);
                first += count;
                self.partial_words += count;
                if self.partial_words == HOP {
                    output.extend_from_slice(&self.partial);
                    self.produced += HOP;
                    self.partial_words = 0;
                }
            }
            self.overlap.copy_within(HOP.., 0);
            self.overlap[SIZE - HOP..].fill(0.0);
        }
        self.ended = end;
        if end && self.produced != target {
            return Err(invalid("LTV drain length disagrees with its input"));
        }
        Tensor::new(output.len(), 1, output)
    }
}
