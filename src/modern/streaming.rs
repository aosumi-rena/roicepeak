use super::{AcousticDecoder, PitchModel, durations};
use crate::{
    Result,
    graph::{
        Budget, Graph,
        streaming::{Packet, Rows, Stream},
    },
    invalid,
    sampling::Sampler,
    tensor::{Tensor, checked_elements},
};

const PACKET_FRAMES: usize = 128;

struct FrameNetwork<'a> {
    pointwise: Stream<'a>,
    convolution: Stream<'a>,
    sampler: crate::sampling::streaming::Stream<'a>,
}

impl<'a> FrameNetwork<'a> {
    fn new(
        pointwise: &'a Graph,
        convolution: &'a Graph,
        sampler: &'a Sampler,
        shape: f32,
        offset: f32,
    ) -> Result<Self> {
        Ok(Self {
            pointwise: pointwise.stream()?,
            convolution: convolution.stream()?,
            sampler: sampler.stream(shape, offset)?,
        })
    }

    fn feed(
        &mut self,
        input: &Tensor,
        noise: &Tensor,
        end: bool,
        budget: &mut Budget,
    ) -> Result<(Packet, Packet)> {
        let features = self.pointwise.feed(Some(input), None, end, false, budget)?;
        let context = self
            .convolution
            .feed(Some(input), None, end, false, budget)?;
        let result = self.sampler.feed(
            features.data.as_ref(),
            features.end,
            context.data.as_ref(),
            context.end,
            Some(noise),
            end,
            budget,
        )?;
        Ok((context, result))
    }
}

pub(crate) struct AcousticPackets<'a> {
    pitch: &'a PitchModel,
    decoder: &'a AcousticDecoder,
    pitch_network: FrameNetwork<'a>,
    decoder_network: FrameNetwork<'a>,
    spectral: Stream<'a>,
    spectral_head: Stream<'a>,
    aux: Stream<'a>,
    aux_head: Stream<'a>,
}

impl<'a> AcousticPackets<'a> {
    pub(crate) fn new(pitch: &'a PitchModel, decoder: &'a AcousticDecoder) -> Result<Self> {
        Ok(Self {
            pitch,
            decoder,
            pitch_network: FrameNetwork::new(
                &pitch.pointwise,
                &pitch.convolution,
                &pitch.sampler,
                0.6,
                0.5,
            )?,
            decoder_network: FrameNetwork::new(
                &decoder.pointwise,
                &decoder.convolution,
                &decoder.sampler,
                0.995,
                1.0,
            )?,
            spectral: decoder.spectral.stream()?,
            spectral_head: decoder.spectral_head.stream()?,
            aux: decoder.aux.stream()?,
            aux_head: decoder.aux_head.stream()?,
        })
    }

    pub(crate) fn render(
        mut self,
        pitch_linguistic: &Tensor,
        linguistic: &Tensor,
        controls: &Tensor,
        styles: (&[f32], &[f32]),
        speakers: (&[f32], &[f32]),
        noise: (&Tensor, &Tensor),
        first_frame: usize,
        budget: &mut Budget,
        mut emit: impl FnMut(&mut Tensor, bool, &mut Budget) -> Result<()>,
    ) -> Result<(Tensor, Tensor)> {
        let counts = durations(controls, linguistic.rows())?;
        let pitch_input = self.pitch.frame_inputs(
            pitch_linguistic,
            controls,
            styles.0,
            speakers.0,
            &counts,
            budget,
        )?;
        let (expanded, position) = self.decoder.frame_content(linguistic, &counts, budget)?;
        let frames = pitch_input.rows();
        if frames == 0 || [expanded.rows(), noise.0.rows(), noise.1.rows()] != [frames; 3] {
            return Err(invalid("acoustic packet frame count"));
        }
        let render_frames = frames
            .checked_sub(first_frame)
            .ok_or_else(|| invalid("acoustic playback starts after the sentence"))?;
        let physical_elements = checked_elements(render_frames, 72)?;
        budget.spend(0, render_frames + physical_elements)?;
        let mut pitch_output = Vec::with_capacity(render_frames);
        let mut physical_output = Vec::with_capacity(physical_elements);
        let mut pending_pitch = Rows::default();
        let mut pending_spectral = Rows::default();
        let mut pending_aux = Rows::default();
        for first in (first_frame..frames).step_by(PACKET_FRAMES) {
            let last = (first + PACKET_FRAMES).min(frames);
            let end = last == frames;
            let (_, pitch) = self.pitch_network.feed(
                &slice(&pitch_input, first, last, budget)?,
                &slice(noise.0, first, last, budget)?,
                end,
                budget,
            )?;
            if pitch.end != end {
                return Err(invalid("pitch stream ended at an unexpected input frame"));
            }
            let pitch = match pitch.data {
                Some(data) if data.cols() == 1 => data,
                None if !end => continue,
                None => Tensor::zeros(0, 1)?,
                _ => return Err(invalid("pitch stream output width")),
            };
            let start = first_frame + pitch_output.len();
            let stop = start + pitch.rows();
            if stop > frames || (end && stop != frames) {
                return Err(invalid("pitch stream output length"));
            }
            pitch_output.extend_from_slice(pitch.values());
            let input = self.decoder.condition_frames(
                &slice(&expanded, start, stop, budget)?,
                &slice(&position, start, stop, budget)?,
                &pitch,
                styles.1,
                speakers.1,
                budget,
            )?;
            let (context, latent) = self.decoder_network.feed(
                &input,
                &slice(noise.1, start, stop, budget)?,
                end,
                budget,
            )?;
            let spectral = self.spectral.feed(
                context.data.as_ref(),
                latent.data.as_ref(),
                context.end,
                latent.end,
                budget,
            )?;
            let spectral = self.spectral_head.feed(
                spectral.data.as_ref(),
                None,
                spectral.end,
                false,
                budget,
            )?;
            let aux = self.aux.feed(
                context.data.as_ref(),
                spectral.data.as_ref(),
                context.end,
                spectral.end,
                budget,
            )?;
            let aux = self
                .aux_head
                .feed(aux.data.as_ref(), None, aux.end, false, budget)?;
            pending_pitch.push(Some(&pitch), end, budget)?;
            pending_spectral.push(spectral.data.as_ref(), spectral.end, budget)?;
            pending_aux.push(aux.data.as_ref(), aux.end, budget)?;
            let rows = pending_pitch
                .rows()
                .min(pending_spectral.rows())
                .min(pending_aux.rows());
            let mut physical = match (
                pending_pitch.take(rows, budget)?,
                pending_spectral.take(rows, budget)?,
                pending_aux.take(rows, budget)?,
            ) {
                (Some(pitch), Some(spectral), Some(aux)) => {
                    self.decoder.physical(&pitch, &spectral, &aux, budget)?
                }
                _ if !end => continue,
                _ => Tensor::zeros(0, 72)?,
            };
            if end && !(pending_pitch.end() && pending_spectral.end() && pending_aux.end()) {
                return Err(invalid("acoustic stream branch lengths differ"));
            }
            if physical_output.len() + physical.values().len() > physical_elements {
                return Err(invalid("acoustic stream output length"));
            }
            emit(&mut physical, end, budget)?;
            physical_output.extend_from_slice(physical.values());
        }
        Ok((
            Tensor::new(render_frames, 1, pitch_output)?,
            Tensor::new(render_frames, 72, physical_output)?,
        ))
    }
}

fn slice(input: &Tensor, first: usize, last: usize, budget: &mut Budget) -> Result<Tensor> {
    let cols = input.cols();
    let values = &input.values()[first * cols..last * cols];
    budget.spend(0, values.len())?;
    Tensor::new(last - first, cols, values.to_vec())
}
