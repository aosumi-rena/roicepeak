//! Complete inference from explicit token trees, without automatic bank discovery.
//! Text analysis is separate; the editor's observed default boundary policy is opt-in.
#[path = "synthesis_query.rs"]
mod query;
pub use query::PreparedQuery;
#[path = "synthesis_duration.rs"]
mod duration;
#[path = "synthesis_intonation.rs"]
mod intonation;

use crate::{
    Result,
    acoustic::LegacyAcoustic,
    dnni::Node,
    graph::Budget,
    invalid,
    language::{CharacterCache, LanguageModel},
    modern::{AcousticDecoder, PitchModel},
    phonemes::{Encoded, PhonemeEncoder, Token},
    prosody::{Prosody, logit},
    sampling::{NormalStream, frame_noise, phoneme_seeds},
    scalar_layers::ScalarLayers,
    stylebook::{Offsets, Style, Stylebook, speaker_vector, split_speakers},
    tensor::Tensor,
    vocoder::{Effects, Vocoder},
};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub struct Parameters {
    pub speed: f32,
    pub pitch: f32,
    pub pause: f32,
    pub volume: f32,
    pub invert_intonation: bool,
    pub fixed_seeding: bool,
}
/// Explicit edits use encoded phone indices and global syllable slots.
#[derive(Clone, Debug, Default)]
pub struct Edits {
    pub accent_codes: Option<Vec<u32>>,
    /// Token/syllable mask: false retains the supplied code, true permits prediction.
    pub predict: Option<Vec<Vec<bool>>>,
    pub seed_variations: Option<Vec<i32>>,
    pub seed_base: i32,
    pub pitch: BTreeMap<usize, f32>,
    pub duration: BTreeMap<usize, f64>,
    pub authored_duration: BTreeMap<usize, DurationEdit>,
    pub intonation_delta: BTreeMap<usize, f32>,
    pub boundary_silence: BoundarySilence,
    /// Native normalized block/project/narrator controls. When set, scalar
    /// Parameters must remain neutral; their boolean options still apply.
    pub scalar_layers: Option<ScalarLayers>,
    pub value_policy: crate::scalar_layers::ValuePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DurationEdit {
    Relative(f64),
    Absolute(f64),
}
impl Edits {
    /// Retained heap charge, excluding the inline Edits itself.
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        m::sum([
            m::optional(&self.accent_codes, m::values),
            m::optional(&self.predict, |rows| m::nested(rows, m::values)),
            m::optional(&self.seed_variations, m::values),
            m::btree(&self.pitch, |_| 0, |_| 0),
            m::btree(&self.duration, |_| 0, |_| 0),
            m::btree(&self.authored_duration, |_| 0, |_| 0),
            m::btree(&self.intonation_delta, |_| 0, |_| 0),
        ])
    }
}
impl DurationEdit {
    pub fn value(self) -> f64 {
        match self {
            Self::Relative(value) | Self::Absolute(value) => value,
        }
    }
}

/// Application behavior is explicit so direct model inference stays unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoundarySilence {
    #[default]
    Predicted,
    /// Observed native editor constructor default, not a universal saved setting.
    NativeEditorDefault,
}
impl Default for Parameters {
    fn default() -> Self {
        Self {
            speed: 100.0,
            pitch: 0.0,
            pause: 100.0,
            volume: 100.0,
            invert_intonation: false,
            fixed_seeding: false,
        }
    }
}
enum Acoustic {
    Legacy(LegacyAcoustic),
    Modern(PitchModel, AcousticDecoder, Vec<f32>, Vec<f32>),
}
pub struct Synthesizer {
    identity: Arc<()>,
    encoder: PhonemeEncoder,
    lm: LanguageModel,
    prosody: Prosody,
    acoustic: Acoustic,
    vocoder: Vocoder,
    stylebook: Stylebook,
    speaker: usize,
    speaker_im: Vec<f32>,
    speaker_accent: Vec<f32>,
}
pub struct Models<'a, 'b> {
    pub phonemes: &'b Node<'a>,
    pub language: &'b Node<'a>,
    pub intonation: &'b Node<'a>,
    pub accent: &'b Node<'a>,
    pub acoustic: &'b Node<'a>,
    pub vocoder: &'b Node<'a>,
    pub stylebook: &'b Node<'a>,
}
pub struct Render {
    pub encoded: Encoded,
    pub raw_query_durations: Vec<f32>,
    pub raw_query_intonation: Vec<f32>,
    /// Interior query phones, before frame quantization, in encoded order.
    /// Direct CLI post-frame duration scales are separate from query timing.
    pub phone_timing: Vec<PhoneTiming>,
    pub stages: BTreeMap<&'static str, Tensor>,
    pub timings: Option<RenderTimings>,
}
/// Controlled query time, separate from physical frame/sample boundaries.
#[derive(Clone, Debug, PartialEq)]
pub struct PhoneTiming {
    pub index: usize,
    pub start: f64,
    pub duration: f64,
}
/// Acoustic controls and the corresponding pre-frame query timing metadata.
pub struct Controls {
    pub values: Tensor,
    pub phone_timing: Vec<PhoneTiming>,
}
/// Wall-clock attribution for one core render, without text analysis or model loading.
#[derive(Clone, Debug, Default)]
pub struct RenderTimings {
    pub encoding: Duration,
    pub language: Duration,
    /// Includes prediction-mask preparation and style mixing before prosody.
    pub prosody: Duration,
    pub controls: Duration,
    /// Includes the acoustic-stage outputs and aperiodicity adjustment.
    pub acoustic: Duration,
    pub vocoder_and_gain: Duration,
    pub total: Duration,
}
impl Synthesizer {
    /// Retained heap charge, excluding the inline Synthesizer itself.
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        let acoustic = match &self.acoustic {
            Acoustic::Legacy(value) => value.estimated_bytes(),
            Acoustic::Modern(pitch, decoder, a, b) => m::sum([
                pitch.estimated_bytes(),
                decoder.estimated_bytes(),
                m::values(a),
                m::values(b),
            ]),
        };
        m::sum([
            m::arc::<()>(0),
            self.encoder.estimated_bytes(),
            self.lm.estimated_bytes(),
            self.prosody.estimated_bytes(),
            acoustic,
            self.vocoder.estimated_bytes(),
            self.stylebook.estimated_bytes(),
            m::values(&self.speaker_im),
            m::values(&self.speaker_accent),
        ])
    }

    pub fn compile(
        models: Models<'_, '_>,
        speaker: usize,
        offsets: Option<Offsets>,
        style_phones: Vec<String>,
        custom_speaker: Option<&[f32]>,
    ) -> Result<Self> {
        let encoder = PhonemeEncoder::compile(models.phonemes)?;
        if models.acoustic.tag == "_ttsna0" && custom_speaker.is_some() {
            return Err(invalid("custom speakers require modern acoustic models"));
        }
        let mut owners: Vec<_> = models.acoustic.children.iter().collect();
        owners.extend([models.intonation, models.accent]);
        let custom = custom_speaker
            .map(|values| split_speakers(values, &owners))
            .transpose()?;
        let vector = |owner: &Node<'_>| match &custom {
            Some(v) => v
                .get(&owner.tag)
                .cloned()
                .ok_or_else(|| invalid("missing custom speaker owner")),
            None => speaker_vector(owner, speaker),
        };
        let acoustic = if models.acoustic.tag == "_ttsna0" {
            Acoustic::Legacy(LegacyAcoustic::compile(models.acoustic)?)
        } else {
            Acoustic::Modern(
                PitchModel::compile(models.acoustic)?,
                AcousticDecoder::compile(models.acoustic)?,
                vector(
                    models
                        .acoustic
                        .children
                        .iter()
                        .find(|n| n.tag.starts_with("_ttsdf"))
                        .ok_or_else(|| invalid("missing pitch owner"))?,
                )?,
                vector(
                    models
                        .acoustic
                        .children
                        .iter()
                        .find(|n| n.tag.starts_with("_ttsdt"))
                        .ok_or_else(|| invalid("missing timbre owner"))?,
                )?,
            )
        };
        Ok(Self {
            identity: Arc::new(()),
            prosody: Prosody::compile(models.intonation, models.accent, encoder.format.clone())?,
            encoder,
            lm: LanguageModel::compile(models.language)?,
            acoustic,
            vocoder: Vocoder::compile(models.vocoder)?,
            stylebook: Stylebook::compile(models.stylebook, offsets, style_phones)?,
            speaker,
            speaker_im: vector(models.intonation)?,
            speaker_accent: vector(models.accent)?,
        })
    }
    pub fn emotions(&self) -> &[String] {
        &self.stylebook.emotions
    }
    pub fn set_vocoder_effects(&mut self, effects: Effects) {
        self.vocoder.set_effects(effects);
        self.identity = Arc::new(());
    }
    pub fn set_character_cache(&mut self, cache: CharacterCache) -> Result<()> {
        self.lm.set_character_cache(cache)?;
        self.identity = Arc::new(());
        Ok(())
    }
    pub fn run(
        &self,
        tokens: &[Token],
        weights: &[f32],
        params: &Parameters,
        budget: &mut Budget,
    ) -> Result<Render> {
        self.run_edited(tokens, weights, params, &Edits::default(), budget)
    }
    pub fn run_edited(
        &self,
        tokens: &[Token],
        weights: &[f32],
        params: &Parameters,
        edits: &Edits,
        budget: &mut Budget,
    ) -> Result<Render> {
        self.run_edited_impl(tokens, weights, params, edits, budget, false)
    }
    /// Render with stage timings; synthesis operations and arithmetic are unchanged.
    pub fn run_edited_profiled(
        &self,
        tokens: &[Token],
        weights: &[f32],
        params: &Parameters,
        edits: &Edits,
        budget: &mut Budget,
    ) -> Result<Render> {
        self.run_edited_impl(tokens, weights, params, edits, budget, true)
    }
    fn run_edited_impl(
        &self,
        tokens: &[Token],
        weights: &[f32],
        params: &Parameters,
        edits: &Edits,
        budget: &mut Budget,
        collect_timings: bool,
    ) -> Result<Render> {
        Self::validate_tokens(tokens)?;
        let scalars = self.validate_parameters(params, edits)?;
        let started = collect_timings.then(Instant::now);
        let prepared = self.prepare_query_impl(tokens, weights, edits, budget, collect_timings)?;
        let mut rendered =
            self.render_prepared_impl(&prepared, params, edits, budget, collect_timings, scalars)?;
        if let (Some(timings), Some(query_timings), Some(started)) =
            (&mut rendered.timings, &prepared.timings, started)
        {
            timings.encoding = query_timings.encoding;
            timings.language = query_timings.language;
            timings.prosody = query_timings.prosody;
            timings.total = started.elapsed();
        }
        Ok(rendered)
    }

    fn validate_parameters(
        &self,
        params: &Parameters,
        edits: &Edits,
    ) -> Result<Option<crate::scalar_layers::ResolvedScalars>> {
        if [params.speed, params.pitch, params.pause, params.volume]
            .iter()
            .any(|v| !v.is_finite())
            || !(50.0..=200.0).contains(&params.speed)
            || !(-150.0..=150.0).contains(&params.pitch)
            || !(50.0..=200.0).contains(&params.pause)
            || !(0.0..=200.0).contains(&params.volume)
        {
            return Err(invalid(
                "synthesis controls outside supported native UI ranges",
            ));
        }
        let scalar_layers = edits
            .scalar_layers
            .map(|layers| layers.resolve_with_policy(edits.value_policy))
            .transpose()?;
        if scalar_layers.is_some()
            && (params.speed != 100.0
                || params.pitch != 0.0
                || params.pause != 100.0
                || params.volume != 100.0)
        {
            return Err(invalid(
                "native scalar layers require neutral scalar Parameters",
            ));
        }
        if !params.fixed_seeding && (edits.seed_variations.is_some() || edits.seed_base != 0) {
            return Err(invalid("seed variations and base require fixed seeding"));
        }
        if matches!(self.acoustic, Acoustic::Legacy(_))
            && (edits.seed_variations.is_some() || edits.seed_base != 0)
        {
            return Err(invalid(
                "seed variation edits require modern acoustic models",
            ));
        }
        Ok(scalar_layers)
    }

    fn render_prepared_impl(
        &self,
        prepared: &PreparedQuery,
        params: &Parameters,
        edits: &Edits,
        budget: &mut Budget,
        collect_timings: bool,
        scalar_layers: Option<crate::scalar_layers::ResolvedScalars>,
    ) -> Result<Render> {
        prepared.validate(self, edits)?;
        let started = collect_timings.then(Instant::now);
        let mut checkpoint = started;
        let mut stage_elapsed = || {
            checkpoint.as_mut().map_or(Duration::ZERO, |previous| {
                let now = Instant::now();
                let elapsed = now.duration_since(*previous);
                *previous = now;
                elapsed
            })
        };
        let x = prepared.copy_encoded();
        let style = &prepared.style;
        let predict = &prepared.prediction_mask;
        let controlled = self.prepared_controls_impl(prepared, params, edits)?;
        let controls = controlled.values;
        let controls_elapsed = stage_elapsed();
        let mut stages = BTreeMap::new();
        if let Some(mask) = predict {
            stages.insert(
                "prediction_mask",
                Tensor::new(mask.len(), 1, mask.iter().map(|v| f32::from(*v)).collect())?,
            );
        }
        stages.insert("language", prepared.language.clone());
        stages.insert("prosody", prepared.prosody.clone());
        let mut physical = match &self.acoustic {
            Acoustic::Legacy(m) => {
                let features = Tensor::new(
                    x.linguistic.rows(),
                    x.linguistic.cols() - 2,
                    (0..x.linguistic.rows())
                        .flat_map(|t| x.linguistic.row(t)[2..].iter().copied())
                        .collect(),
                )?;
                m.run(&features, &controls, &style.embedding, self.speaker, budget)?
            }
            Acoustic::Modern(pitch, decoder, pitch_speaker, decoder_speaker) => {
                let counts: Vec<_> = controls
                    .values()
                    .chunks_exact(3)
                    .map(|r| r[2] as usize)
                    .collect();
                let seeds = if params.fixed_seeding {
                    let zeros;
                    let variations = if let Some(values) = &edits.seed_variations {
                        values.as_slice()
                    } else {
                        zeros = vec![0; x.symbols.len()];
                        &zeros
                    };
                    Some(phoneme_seeds(&x.symbols, variations, edits.seed_base)?)
                } else {
                    None
                };
                let mut pitch_input = x.linguistic.clone();
                for t in 0..pitch_input.rows() {
                    for flag in &self.encoder.format.slots[1..3] {
                        pitch_input.row_mut(t)[flag + 2] = 0.0;
                    }
                }
                let noise = frame_noise(
                    &counts,
                    pitch.noise_channels(),
                    0.5,
                    seeds.as_deref(),
                    &mut NormalStream::default(),
                    budget,
                )?;
                let f0 = pitch.run_with_speaker(
                    &pitch_input,
                    &controls,
                    &style.embedding,
                    pitch_speaker,
                    Some(&noise),
                    budget,
                )?;
                let noise = frame_noise(
                    &counts,
                    decoder.noise_channels(),
                    0.75,
                    seeds.as_deref(),
                    &mut NormalStream::default(),
                    budget,
                )?;
                let physical = decoder.run_with_speaker(
                    &x.linguistic,
                    &controls,
                    &f0,
                    &style.embedding,
                    decoder_speaker,
                    Some(&noise),
                    budget,
                )?;
                stages.insert("pitch", f0);
                physical
            }
        };
        if let Some(bap) = style.aperiodicity {
            for t in 0..physical.rows() {
                for (v, offset) in physical.row_mut(t)[67..72].iter_mut().zip(bap) {
                    *v = (*v + offset).clamp(0.0, 1.0);
                }
            }
        }
        let acoustic_elapsed = stage_elapsed();
        let rendered = self
            .vocoder
            .run_with_tension(&physical, style.tension, budget)?;
        let mut pcm = rendered
            .stages
            .get("pcm")
            .ok_or_else(|| invalid("vocoder did not return PCM"))?
            .clone();
        if let Some(scalars) = scalar_layers {
            let gain = style.gain() as f32;
            if !gain.is_finite() {
                return Err(invalid("style volume gain overflow"));
            }
            for t in 0..pcm.rows() {
                // Apply application gain after style gain, then store as float32.
                let sample = pcm.row_mut(t);
                sample[0] *= gain;
                sample[0] = (f64::from(sample[0]) * scalars.volume_gain) as f32;
            }
        } else {
            let gain = style.gain() as f32 * (params.volume / 100.0);
            if !gain.is_finite() {
                return Err(invalid("output volume gain overflow"));
            }
            for t in 0..pcm.rows() {
                pcm.row_mut(t)[0] *= gain;
            }
        }
        pcm.ensure_finite()?;
        let vocoder_and_gain_elapsed = stage_elapsed();
        stages.insert("controls", controls);
        stages.insert("physical", physical);
        stages.insert("pcm", pcm);
        let timings = started.map(|started| RenderTimings {
            encoding: Duration::ZERO,
            language: Duration::ZERO,
            prosody: Duration::ZERO,
            controls: controls_elapsed,
            acoustic: acoustic_elapsed,
            vocoder_and_gain: vocoder_and_gain_elapsed,
            total: started.elapsed(),
        });
        Ok(Render {
            encoded: x,
            raw_query_durations: prepared.raw_query_durations.clone(),
            raw_query_intonation: prepared.intonation_coordinates(params.invert_intonation),
            phone_timing: controlled.phone_timing,
            stages,
            timings,
        })
    }
}
fn prepare_controls(
    x: &Encoded,
    predicted: &Tensor,
    style: &Style,
    raw_durations: &[f32],
    period: f32,
    params: &Parameters,
    edits: &Edits,
) -> Result<Controls> {
    let n = x.linguistic.rows();
    if predicted.rows() != n || predicted.cols() != 3 || !period.is_finite() || period <= 0.0 {
        return Err(invalid("prosody control dimensions"));
    }
    let scalar_layers = edits
        .scalar_layers
        .map(|layers| layers.resolve_with_policy(edits.value_policy))
        .transpose()?;
    let mut intonation = Vec::with_capacity(n);
    for t in 0..n {
        intonation.push(1.0 / (1.0 + (-(logit(predicted.row(t)[1]) + style.intonation)).exp()));
    }
    let duration::Plan {
        mut frames,
        mut seconds,
    } = duration::plan(
        x,
        raw_durations,
        period,
        edits,
        scalar_layers.map_or_else(
            || 1.0 / f64::from(params.speed / 100.0),
            |v| v.speed_duration_factor,
        ),
        scalar_layers.map_or_else(
            || f64::from(params.pause / 100.0),
            |v| v.pause_duration_factor,
        ),
    )?;
    let slots = x.references.iter().map(|r| r[3]).max().unwrap_or(0) + 1;
    if edits
        .intonation_delta
        .iter()
        .any(|(slot, value)| *slot >= slots || !value.is_finite())
        || edits
            .pitch
            .iter()
            .any(|(slot, value)| *slot >= slots || !value.is_finite())
        || edits
            .duration
            .iter()
            .any(|(phone, value)| *phone >= n || !value.is_finite())
    {
        return Err(invalid("invalid synthesis edit index or value"));
    }
    for (phone, scale) in &edits.duration {
        let count = (f64::from(frames[*phone]) * scale)
            .round_ties_even()
            .max(1.0);
        if !count.is_finite() || count > 1_000_000.0 {
            return Err(invalid("edited duration exceeds limit"));
        }
        frames[*phone] = count as f32;
    }
    if edits.boundary_silence == BoundarySilence::NativeEditorDefault {
        // Replace the leading duration after control edits, before acoustics.
        // Only the qualified 5-ms model period is accepted.
        if period.to_bits() != 0.005f32.to_bits()
            || x.symbols.first().map(String::as_str) != Some("sil")
            || x.nucleus.first() != Some(&false)
            || x.references
                .first()
                .is_none_or(|reference| reference[0] != 0)
        {
            return Err(invalid(
                "native editor default silence requires a 5-ms BOS silence phone",
            ));
        }
        let leading_seconds = f32::from_bits(0x3d4c_cccd);
        frames[0] = (f64::from(leading_seconds) / f64::from(period) + 0.5).floor() as f32;
        // The query clock retains the original f32 boundary seconds, rather
        // than reconstructing them from the rounded frame count.
        seconds[0] = leading_seconds;
    }
    let mut user = vec![0.5f32; slots];
    for (i, r) in x.references.iter().enumerate() {
        if x.nucleus[i] {
            user[r[3]] = if params.invert_intonation {
                1.0 - intonation[i]
            } else {
                intonation[i]
            };
        }
    }
    for (slot, value) in &edits.pitch {
        user[*slot] = *value;
    }
    // Editor values are first written through the engine's clamped slot interface.
    // Uncarried slots remain neutral when the application reconstructs its curve.
    if !edits.pitch.is_empty() {
        for (i, r) in x.references.iter().enumerate() {
            if x.nucleus[i] {
                let value = user[r[3]].clamp(0.0, 1.0);
                intonation[i] = if params.invert_intonation {
                    1.0 - value
                } else {
                    value
                };
            }
        }
        user.fill(0.5);
        for (i, r) in x.references.iter().enumerate() {
            if x.nucleus[i] {
                user[r[3]] = if params.invert_intonation {
                    1.0 - intonation[i]
                } else {
                    intonation[i]
                };
            }
        }
    }
    let mean = user.iter().map(|v| f64::from(*v)).sum::<f64>() / slots as f64;
    let variance = user
        .iter()
        .map(|v| (f64::from(*v) - mean).powi(2))
        .sum::<f64>()
        / slots as f64;
    if variance > 0.0 {
        let deviation = variance.sqrt();
        let last = slots.saturating_sub(1);
        for (i, v) in user.iter_mut().enumerate() {
            let z = if i > 0 && i < last {
                ((f64::from(*v) - mean) / deviation) as f32
            } else {
                *v
            };
            let z = edits.intonation_delta.get(&i).map_or(z, |delta| *delta + z);
            *v = (f64::from(z.clamp(-3.0, 3.0)) * deviation + mean) as f32;
        }
    }
    let offset = scalar_layers.map_or_else(
        || (params.pitch / 100.0) * (1.0f32 / 12.0),
        |scalars| scalars.pitch_offset,
    );
    for (i, r) in x.references.iter().enumerate() {
        if x.nucleus[i] {
            let v = (user[r[3]] + offset).clamp(0.0, 1.0);
            intonation[i] = if params.invert_intonation { 1.0 - v } else { v };
        }
    }
    let mut clock = seconds.first().copied().map(f64::from).unwrap_or(0.0);
    let mut phone_timing = Vec::with_capacity(n.saturating_sub(2));
    for (index, &duration) in seconds.iter().enumerate().take(n.saturating_sub(1)).skip(1) {
        let duration = f64::from(duration);
        let duration = if edits.value_policy.allow_negative {
            duration.max(0.0)
        } else {
            duration
        };
        phone_timing.push(PhoneTiming {
            index,
            start: clock,
            duration,
        });
        clock += duration;
    }
    let values = Tensor::new(
        n,
        3,
        (0..n)
            .flat_map(|i| [intonation[i], f32::from(x.nucleus[i]), frames[i]])
            .collect(),
    )?;
    Ok(Controls {
        values,
        phone_timing,
    })
}
