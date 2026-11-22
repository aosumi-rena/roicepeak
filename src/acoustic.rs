//! Acoustic stages on caller-supplied phoneme features and durations.
mod refinement;
mod spectral_filter;

use crate::{
    Result,
    dnni::Node,
    graph::{Budget, Graph},
    invalid,
    normalization::Normalizer,
    sequence::{broadcast, concatenate, cosine_bank, repeat},
    tensor::{Tensor, checked_elements, u32_at},
};

const PACKET_FRAMES: usize = 64;
type EmitPhysical<'a> = dyn FnMut(&mut Tensor, bool, usize, &mut Budget) -> Result<()> + 'a;

pub struct LegacyAcoustic {
    encoder: Graph,
    convolution: Graph,
    speakers: Tensor,
    context: Graph,
    post: Graph,
    recurrent: Graph,
    prenet: Graph,
    head: Graph,
    refinement: refinement::Refinement,
    spectral_filter: spectral_filter::SpectralFilter,
    normalizer: Normalizer,
    idct: Tensor,
    pub frame_period: f32,
}
impl LegacyAcoustic {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.encoder.estimated_bytes(),
            self.convolution.estimated_bytes(),
            self.speakers.estimated_bytes(),
            self.context.estimated_bytes(),
            self.post.estimated_bytes(),
            self.recurrent.estimated_bytes(),
            self.prenet.estimated_bytes(),
            self.head.estimated_bytes(),
            self.refinement.estimated_bytes(),
            self.spectral_filter.estimated_bytes(),
            self.normalizer.estimated_bytes(),
            self.idct.estimated_bytes(),
        ])
    }

    pub fn compile(root: &Node<'_>) -> Result<Self> {
        if root.tag != "_ttsna0" || root.children.len() != 11 {
            return Err(invalid("legacy acoustic root"));
        }
        let c = &root.children;
        let decoder = &c[8];
        let cepstrum = &c[10];
        if c[2].tag != "modl4"
            || c[2].children.len() != 1
            || decoder.tag != "_ardev0"
            || decoder.children.len() != 3
            || decoder.children[0].children.len() != 1
            || decoder.children[0].children[0].children.len() != 1
            || ![3, 4].contains(&cepstrum.children.len())
            || cepstrum.children[1].children.len() != 2
        {
            return Err(invalid("legacy acoustic subgraphs"));
        }
        let dct = &cepstrum.children[1].children[1];
        if dct.payload.len() != 8 + 64 * 64 * 4 {
            return Err(invalid("legacy IDCT dimensions"));
        }
        let idct = Tensor::new(
            64,
            64,
            dct.payload[8..]
                .chunks_exact(4)
                .map(|x| f32::from_le_bytes(x.try_into().unwrap()))
                .collect(),
        )?;
        let frame_period = f32::from_bits(u32_at(cepstrum.payload, 4)?);
        if !frame_period.is_finite() || frame_period <= 0.0 {
            return Err(invalid("legacy frame period"));
        }
        let recurrent = Graph::compile(&decoder.children[0].children[0].children[0])?;
        recurrent.recurrent_state_len()?;
        Ok(Self {
            encoder: Graph::compile(&c[0])?,
            convolution: Graph::compile(&c[1])?,
            speakers: c[2].children[0].tensor()?,
            context: Graph::compile(&c[3])?,
            post: Graph::compile(&c[4])?,
            recurrent,
            prenet: Graph::compile(&decoder.children[1])?,
            head: Graph::compile(&decoder.children[2])?,
            refinement: refinement::Refinement::compile(&c[6], &c[5], &c[7])?,
            spectral_filter: spectral_filter::SpectralFilter::compile(&c[9])?,
            normalizer: Normalizer::from_node(&cepstrum.children[2])?,
            idct,
            frame_period,
        })
    }
    /// Controls: intonation, nucleus flag (0/1), whole duration in 5-ms frames.
    pub fn run(
        &self,
        features: &Tensor,
        controls: &Tensor,
        style: &[f32],
        speaker: usize,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        self.run_impl(features, controls, style, speaker, 0, budget, None)
    }

    pub(crate) fn run_streamed(
        &self,
        features: &Tensor,
        controls: &Tensor,
        style: &[f32],
        speaker: usize,
        first_frame: usize,
        budget: &mut Budget,
        emit: &mut EmitPhysical<'_>,
    ) -> Result<Tensor> {
        self.run_impl(
            features,
            controls,
            style,
            speaker,
            first_frame,
            budget,
            Some(emit),
        )
    }

    fn run_impl(
        &self,
        features: &Tensor,
        controls: &Tensor,
        style: &[f32],
        speaker: usize,
        first_frame: usize,
        budget: &mut Budget,
        mut emit: Option<&mut EmitPhysical<'_>>,
    ) -> Result<Tensor> {
        if features.rows() != controls.rows()
            || controls.cols() != 3
            || speaker >= self.speakers.cols()
        {
            return Err(invalid("legacy acoustic input dimensions"));
        }
        let mut values = Vec::with_capacity(controls.rows());
        let mut counts = Vec::with_capacity(controls.rows());
        for i in 0..controls.rows() {
            let r = controls.row(i);
            if r[1] != 0.0 && r[1] != 1.0 {
                return Err(invalid("nucleus flag must be 0 or 1"));
            }
            values.push(if r[1] == 1.0 { r[0] } else { 0.5 });
            if r[2] < 0.0 || r[2] > 1_000_000.0 || r[2].fract() != 0.0 {
                return Err(invalid("duration must be a bounded nonnegative integer"));
            }
            counts.push(r[2] as usize);
        }
        let hidden = self.encoder.run_with(features, budget)?;
        let bank = cosine_bank(&values, 8, budget)?;
        let style = broadcast(style, features.rows(), budget)?;
        let phones = self
            .convolution
            .run_with(&concatenate(&[&hidden, &bank, &style], budget)?, budget)?;
        let expanded = repeat(&phones, &counts, budget)?;
        let speaker: Vec<_> = (0..self.speakers.rows())
            .map(|i| self.speakers.row(i)[speaker])
            .collect();
        let speaker = broadcast(&speaker, expanded.rows(), budget)?;
        let row = concatenate(&[&expanded, &speaker], budget)?;
        let context = self
            .post
            .run_with(&self.context.run_with(&row, budget)?, budget)?;
        let frames = context
            .rows()
            .checked_sub(first_frame)
            .ok_or_else(|| invalid("acoustic playback starts after the sentence"))?;
        let packet_frames = if emit.is_some() {
            PACKET_FRAMES
        } else {
            frames.max(1)
        };
        let mut state = vec![0.0; self.recurrent.recurrent_state_len()?];
        let mut previous = Tensor::zeros(1, 72)?;
        let mut refinement = self.refinement.stream()?;
        let mut spectral_filter = self.spectral_filter.stream();
        let mut physical = Vec::new();
        if frames > packet_frames {
            let elements = checked_elements(frames, 72)?;
            budget.spend(0, elements)?;
            physical.reserve_exact(elements);
        }
        if frames == 0 {
            return self.physical(&Tensor::zeros(0, 72)?, budget);
        }
        for first in (first_frame..context.rows()).step_by(packet_frames) {
            let stop = (first + packet_frames).min(context.rows());
            let rows = stop - first;
            let width = context.cols();
            let hidden_width = state.len();
            budget.spend(0, rows.saturating_mul(72 + hidden_width + 2 * width))?;
            let condition = Tensor::new(
                rows,
                width,
                context.values()[first * width..stop * width].to_vec(),
            )?;
            let mut normalized = Tensor::zeros(rows, 72)?;
            let mut hidden = Tensor::zeros(rows, hidden_width)?;
            for frame in 0..rows {
                let row = Tensor::new(1, width, condition.row(frame).to_vec())?;
                let pre = self.prenet.run_with(&previous, budget)?;
                let row = concatenate(&[&row, &pre], budget)?;
                let decoded = self.recurrent.run_recurrent(&row, &mut state, budget)?;
                if decoded.rows() != 1 || decoded.cols() != hidden_width {
                    return Err(invalid("autoregressive hidden channels differ"));
                }
                previous = self.head.run_with(&decoded, budget)?;
                if previous.rows() != 1 || previous.cols() != 72 {
                    return Err(invalid("autoregressive output must have 72 channels"));
                }
                hidden.row_mut(frame).copy_from_slice(decoded.row(0));
                normalized.row_mut(frame).copy_from_slice(previous.row(0));
            }
            let end = stop == context.rows();
            let refined = refinement.feed(&condition, &hidden, &normalized, end, budget)?;
            if refined.end != end {
                return Err(invalid("legacy acoustic refinement did not finish"));
            }
            let Some(filtered) = spectral_filter.feed(refined.data.as_ref(), end, budget)? else {
                continue;
            };
            let mut packet = self.physical(&filtered, budget)?;
            if let Some(emit) = &mut emit {
                emit(&mut packet, end, frames, budget)?;
            }
            if first == first_frame && stop == context.rows() {
                return Ok(packet);
            }
            physical.extend_from_slice(packet.values());
        }
        Tensor::new(frames, 72, physical)
    }

    fn physical(&self, normalized: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        let mut physical = self.normalizer.denormalize(normalized)?;
        if physical.cols() != 72 {
            return Err(invalid("physical feature width must be 72"));
        }
        budget.spend(physical.rows().saturating_mul(64 * 64), 0)?;
        for t in 0..physical.rows() {
            physical.row_mut(t)[0] = f32::from(physical.row(t)[0] > 0.5);
            let input = physical.row(t)[3..67].to_vec();
            for o in 0..64 {
                physical.row_mut(t)[3 + o] =
                    input.iter().zip(self.idct.row(o)).map(|(a, b)| a * b).sum();
            }
        }
        physical.ensure_finite()?;
        Ok(physical)
    }
}
