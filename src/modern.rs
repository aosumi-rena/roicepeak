//! Modern pitch and timbre stages. Inputs remain explicit and independently comparable.
pub(crate) mod streaming;
use crate::{
    Result,
    dnni::Node,
    graph::{Budget, Graph},
    invalid,
    lingual::Lingual,
    normalization::Normalizer,
    sampling::{NoiseKind, NormalStream, Sampler, frame_noise},
    sequence::{broadcast, concatenate, cosine_bank, positions, repeat},
    tensor::{MAX_ELEMENTS, Tensor, u32_at},
};

fn stage<'a, 'b>(root: &'b Node<'a>, tags: &[&str]) -> Result<&'b Node<'a>> {
    if !["_ttscs1", "_ttscs2"].contains(&root.tag.as_str()) {
        return Err(invalid("modern acoustic root"));
    }
    let mut found = root
        .children
        .iter()
        .filter(|n| tags.contains(&n.tag.as_str()));
    let n = found
        .next()
        .ok_or_else(|| invalid("missing modern acoustic stage"))?;
    if found.next().is_some() {
        return Err(invalid("duplicate acoustic stage"));
    }
    Ok(n)
}
fn speaker_table(n: &Node<'_>) -> Result<Tensor> {
    if n.tag != "modl4" || n.children.len() != 1 {
        return Err(invalid("speaker table structure"));
    }
    n.children[0].tensor()
}
fn speaker(table: &Tensor, index: usize, channels: usize) -> Result<Vec<f32>> {
    if index >= table.cols() || channels != table.rows() {
        return Err(invalid("speaker vector dimensions"));
    }
    Ok((0..channels).map(|i| table.row(i)[index]).collect())
}
fn durations(controls: &Tensor, rows: usize) -> Result<Vec<usize>> {
    if controls.rows() != rows || controls.cols() != 3 {
        return Err(invalid(
            "controls must have rows [intonation,nucleus,duration]",
        ));
    }
    controls
        .values()
        .chunks_exact(3)
        .map(|r| {
            if r[2] < 0.0
                || r[2] > 1_000_000.0
                || r[2].fract() != 0.0
                || (r[1] != 0.0 && r[1] != 1.0)
            {
                Err(invalid("invalid acoustic duration/nucleus"))
            } else {
                Ok(r[2] as usize)
            }
        })
        .collect()
}
pub struct PitchModel {
    pub lingual: Lingual,
    speakers: Tensor,
    speaker_channels: usize,
    style_channels: usize,
    bank_channels: usize,
    recurrent: Graph,
    phone_stack: Graph,
    pointwise: Graph,
    convolution: Graph,
    sampler: Sampler,
    acoustic: Normalizer,
    noise_kind: NoiseKind,
}
impl PitchModel {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.lingual.estimated_bytes(),
            self.speakers.estimated_bytes(),
            self.recurrent.estimated_bytes(),
            self.phone_stack.estimated_bytes(),
            self.pointwise.estimated_bytes(),
            self.convolution.estimated_bytes(),
            self.sampler.estimated_bytes(),
            self.acoustic.estimated_bytes(),
        ])
    }

    pub fn noise_channels(&self) -> usize {
        self.sampler.channels
    }
    pub(crate) fn noise_stream(&self) -> NormalStream {
        NormalStream::with_kind(self.noise_kind)
    }
    pub fn compile(root: &Node<'_>) -> Result<Self> {
        let n = stage(root, &["_ttsdf0", "_ttsdf1"])?;
        if n.children.len() != 3 {
            return Err(invalid("pitch stage children"));
        }
        let e = &n.children[1];
        if e.payload.len() != 12 || e.children.len() != 6 {
            return Err(invalid("pitch encoder structure"));
        }
        let c = &e.children;
        let stats = c[5]
            .children
            .iter()
            .filter(|n| n.tag == "cmpu0")
            .collect::<Vec<_>>();
        if stats.is_empty() {
            return Err(invalid("pitch acoustic statistics"));
        }
        Ok(Self {
            lingual: Lingual::compile(&c[0])?,
            speakers: speaker_table(&n.children[0])?,
            speaker_channels: u32_at(e.payload, 0)? as usize,
            style_channels: u32_at(e.payload, 4)? as usize,
            bank_channels: u32_at(e.payload, 8)? as usize,
            recurrent: Graph::compile(&c[1])?,
            phone_stack: Graph::compile(&c[2])?,
            pointwise: Graph::compile(&c[3])?,
            convolution: Graph::compile(&c[4])?,
            sampler: Sampler::compile(&n.children[2])?,
            acoustic: Normalizer::from_node(stats[0])?,
            noise_kind: NoiseKind::for_stage(&n.tag),
        })
    }
    pub fn run(
        &self,
        linguistic: &Tensor,
        controls: &Tensor,
        style: &[f32],
        speaker_id: usize,
        noise: Option<&Tensor>,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let vector = speaker(&self.speakers, speaker_id, self.speaker_channels)?;
        self.run_with_speaker(linguistic, controls, style, &vector, noise, budget)
    }
    pub fn run_with_speaker(
        &self,
        linguistic: &Tensor,
        controls: &Tensor,
        style: &[f32],
        speaker_vector: &[f32],
        noise: Option<&Tensor>,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let counts = durations(controls, linguistic.rows())?;
        let input =
            self.frame_inputs(linguistic, controls, style, speaker_vector, &counts, budget)?;
        let features = self.pointwise.run_with(&input, budget)?;
        let context = self.convolution.run_with(&input, budget)?;
        let generated;
        let noise = if let Some(n) = noise {
            n
        } else {
            generated = frame_noise(
                &counts,
                self.sampler.channels,
                0.5,
                None,
                &mut self.noise_stream(),
                budget,
            )?;
            &generated
        };
        self.sampler
            .run(&features, &context, noise, 0.6, 0.5, budget)
    }

    fn frame_inputs(
        &self,
        linguistic: &Tensor,
        controls: &Tensor,
        style: &[f32],
        speaker_vector: &[f32],
        counts: &[usize],
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if speaker_vector.len() != self.speaker_channels
            || speaker_vector.iter().any(|v| !v.is_finite())
        {
            return Err(invalid("pitch speaker vector dimensions"));
        }
        if style.len() != self.style_channels {
            return Err(invalid("pitch style width"));
        }
        let values = controls
            .values()
            .chunks_exact(3)
            .map(|r| if r[1] == 1.0 { r[0] } else { 0.5 })
            .collect::<Vec<_>>();
        let encoded = self.lingual.run(linguistic, budget)?;
        let hidden = self.recurrent.run_with(&encoded, budget)?;
        let bank = cosine_bank(&values, self.bank_channels, budget)?;
        let style = broadcast(style, linguistic.rows(), budget)?;
        let phone_input = concatenate(&[&hidden, &bank, &style], budget)?;
        let encoded = self.phone_stack.run_with(&phone_input, budget)?;
        let expanded = repeat(&encoded, counts, budget)?;
        let bank = cosine_bank(&positions(counts)?, 4, budget)?;
        let speaker = broadcast(speaker_vector, expanded.rows(), budget)?;
        concatenate(&[&expanded, &bank, &speaker], budget)
    }
    pub fn to_hz(&self, normalized: &Tensor) -> Result<Tensor> {
        self.acoustic.channels(1..2)?.denormalize(normalized)
    }
}

pub struct AcousticDecoder {
    pub lingual: Lingual,
    speakers: Tensor,
    speaker_channels: usize,
    style_channels: usize,
    pointwise: Graph,
    convolution: Graph,
    sampler: Sampler,
    spectral: Graph,
    spectral_head: Graph,
    aux: Graph,
    aux_head: Graph,
    normalizer: Normalizer,
    pub frame_period: f32,
    noise_kind: NoiseKind,
}

#[derive(Clone, Copy)]
pub(crate) struct StyleLayout {
    channels: usize,
    separate: bool,
}

impl StyleLayout {
    pub(crate) fn compile(
        root: &Node<'_>,
        pitch: &PitchModel,
        decoder: &AcousticDecoder,
    ) -> Result<Self> {
        Self::from_channels(root, pitch.style_channels, decoder.style_channels)
    }

    fn from_channels(root: &Node<'_>, pitch: usize, decoder: usize) -> Result<Self> {
        if !["_ttscs1", "_ttscs2"].contains(&root.tag.as_str()) {
            return Err(invalid("modern acoustic root"));
        }
        if pitch != decoder {
            return Err(invalid("pitch and timbre style widths differ"));
        }
        let separate = root.tag == "_ttscs2";
        if pitch
            .checked_mul(if separate { 2 } else { 1 })
            .is_none_or(|channels| channels > MAX_ELEMENTS)
        {
            return Err(invalid("model style width exceeds tensor budget"));
        }
        Ok(Self {
            channels: pitch,
            separate,
        })
    }

    pub(crate) fn channels(self) -> usize {
        self.channels * if self.separate { 2 } else { 1 }
    }

    pub(crate) fn split(self, values: &[f32]) -> Result<(&[f32], &[f32])> {
        if values.len() != self.channels() {
            return Err(invalid("model style embedding width"));
        }
        if self.separate {
            Ok(values.split_at(self.channels))
        } else {
            Ok((values, values))
        }
    }
}

pub fn style_embedding_channels(root: &Node<'_>) -> Result<usize> {
    let pitch = stage(root, &["_ttsdf0", "_ttsdf1"])?;
    let decoder = stage(root, &["_ttsdt0", "_ttsdt1"])?;
    if pitch.children.len() != 3 || decoder.children.len() != 8 {
        return Err(invalid("modern acoustic stage children"));
    }
    let pitch = &pitch.children[1];
    let decoder = &decoder.children[2];
    if pitch.payload.len() != 12
        || pitch.children.len() != 6
        || decoder.payload.len() != 8
        || decoder.children.len() != 3
    {
        return Err(invalid("modern style conditioning structure"));
    }
    Ok(StyleLayout::from_channels(
        root,
        u32_at(pitch.payload, 4)? as usize,
        u32_at(decoder.payload, 4)? as usize,
    )?
    .channels())
}

impl AcousticDecoder {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.lingual.estimated_bytes(),
            self.speakers.estimated_bytes(),
            self.pointwise.estimated_bytes(),
            self.convolution.estimated_bytes(),
            self.sampler.estimated_bytes(),
            self.spectral.estimated_bytes(),
            self.spectral_head.estimated_bytes(),
            self.aux.estimated_bytes(),
            self.aux_head.estimated_bytes(),
            self.normalizer.estimated_bytes(),
        ])
    }

    pub fn noise_channels(&self) -> usize {
        self.sampler.channels
    }
    pub(crate) fn noise_stream(&self) -> NormalStream {
        NormalStream::with_kind(self.noise_kind)
    }
    pub fn compile(root: &Node<'_>) -> Result<Self> {
        let n = stage(root, &["_ttsdt0", "_ttsdt1"])?;
        if n.children.len() != 8 {
            return Err(invalid("timbre stage children"));
        }
        let c = &n.children;
        let front = &c[0];
        let context = &c[2];
        if front.children.len() < 3 || context.children.len() != 3 || context.payload.len() != 8 {
            return Err(invalid("timbre stage structure"));
        }
        let frame_period = f32::from_bits(u32_at(front.payload, 4)?);
        if !frame_period.is_finite() || frame_period <= 0.0 {
            return Err(invalid("timbre frame period"));
        }
        Ok(Self {
            lingual: Lingual::compile(&context.children[0])?,
            speakers: speaker_table(&c[1])?,
            speaker_channels: u32_at(context.payload, 0)? as usize,
            style_channels: u32_at(context.payload, 4)? as usize,
            pointwise: Graph::compile(&context.children[1])?,
            convolution: Graph::compile(&context.children[2])?,
            sampler: Sampler::compile(&c[3])?,
            spectral: Graph::compile(&c[4])?,
            spectral_head: Graph::compile(&c[5])?,
            aux: Graph::compile(&c[6])?,
            aux_head: Graph::compile(&c[7])?,
            normalizer: Normalizer::from_node(&front.children[2])?,
            frame_period,
            noise_kind: NoiseKind::for_stage(&n.tag),
        })
    }
    pub fn normalize_hz(&self, hz: &Tensor) -> Result<Tensor> {
        self.normalizer.channels(1..2)?.normalize(hz, false)
    }
    pub fn run(
        &self,
        linguistic: &Tensor,
        controls: &Tensor,
        pitch: &Tensor,
        style: &[f32],
        speaker_id: usize,
        noise: Option<&Tensor>,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let vector = speaker(&self.speakers, speaker_id, self.speaker_channels)?;
        self.run_with_speaker(linguistic, controls, pitch, style, &vector, noise, budget)
    }
    pub fn run_with_speaker(
        &self,
        linguistic: &Tensor,
        controls: &Tensor,
        pitch: &Tensor,
        style: &[f32],
        speaker_vector: &[f32],
        noise: Option<&Tensor>,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        let counts = durations(controls, linguistic.rows())?;
        let (expanded, position) = self.frame_content(linguistic, &counts, budget)?;
        let input =
            self.condition_frames(&expanded, &position, pitch, style, speaker_vector, budget)?;
        let context = self.convolution.run_with(&input, budget)?;
        let features = self.pointwise.run_with(&input, budget)?;
        let generated;
        let noise = if let Some(n) = noise {
            n
        } else {
            generated = frame_noise(
                &counts,
                self.sampler.channels,
                0.75,
                None,
                &mut self.noise_stream(),
                budget,
            )?;
            &generated
        };
        let latent = self
            .sampler
            .run(&features, &context, noise, 0.995, 1.0, budget)?;
        let spectral = self.spectral_head.run_with(
            &self.spectral.run_conditioned(&context, &latent, budget)?,
            budget,
        )?;
        let aux = self.aux_head.run_with(
            &self.aux.run_conditioned(&context, &spectral, budget)?,
            budget,
        )?;
        self.physical(pitch, &spectral, &aux, budget)
    }

    fn frame_content(
        &self,
        linguistic: &Tensor,
        counts: &[usize],
        budget: &mut Budget,
    ) -> Result<(Tensor, Tensor)> {
        let encoded = self.lingual.run(linguistic, budget)?;
        let expanded = repeat(&encoded, counts, budget)?;
        let position = cosine_bank(&positions(counts)?, 4, budget)?;
        Ok((expanded, position))
    }

    fn condition_frames(
        &self,
        expanded: &Tensor,
        position: &Tensor,
        pitch: &Tensor,
        style: &[f32],
        speaker_vector: &[f32],
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if speaker_vector.len() != self.speaker_channels
            || speaker_vector.iter().any(|v| !v.is_finite())
            || style.len() != self.style_channels
            || pitch.cols() != 1
            || expanded.rows() != pitch.rows()
        {
            return Err(invalid("timbre conditioning dimensions"));
        }
        let pitch_bank = cosine_bank(pitch.values(), 4, budget)?;
        let speaker = broadcast(speaker_vector, expanded.rows(), budget)?;
        let style = broadcast(style, expanded.rows(), budget)?;
        concatenate(&[expanded, position, &pitch_bank, &speaker, &style], budget)
    }

    fn physical(
        &self,
        pitch: &Tensor,
        spectral: &Tensor,
        aux: &Tensor,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if spectral.cols() != 64
            || aux.cols() != 7
            || spectral.rows() != pitch.rows()
            || aux.rows() != pitch.rows()
        {
            return Err(invalid(
                "timbre output must have 64 spectral and 7 auxiliary channels",
            ));
        }
        let mut normalized = Tensor::zeros(pitch.rows(), 72)?;
        budget.spend(0, normalized.values().len())?;
        for t in 0..pitch.rows() {
            let r = normalized.row_mut(t);
            r[0] = aux.row(t)[0];
            r[1] = pitch.row(t)[0];
            r[2] = aux.row(t)[1];
            r[3..67].copy_from_slice(spectral.row(t));
            r[67..].copy_from_slice(&aux.row(t)[2..]);
        }
        let mut physical = self.normalizer.denormalize(&normalized)?;
        for t in 0..physical.rows() {
            let r = physical.row_mut(t);
            r[0] = if r[0] > 0.5 { 1.0 } else { 0.0 };
            r[2] = r[2].clamp(0.01, 3.0);
            for x in &mut r[67..] {
                *x = x.clamp(0.0, 1.0);
            }
        }
        Ok(physical)
    }
}
