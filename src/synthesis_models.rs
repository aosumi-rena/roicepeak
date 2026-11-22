use super::{Models, Synthesizer};
use crate::{
    Result,
    acoustic::LegacyAcoustic,
    dnni::Node,
    invalid,
    language::LanguageModel,
    memory as m,
    modern::{AcousticDecoder, PitchModel, StyleLayout},
    phonemes::PhonemeEncoder,
    prosody::Prosody,
    stylebook::{Offsets, Stylebook, speaker_vector, split_speakers},
    vocoder::Vocoder,
};
use std::{collections::BTreeSet, sync::Arc};

pub(super) enum Acoustic {
    Legacy(LegacyAcoustic),
    Modern(PitchModel, AcousticDecoder),
}

#[derive(Clone)]
pub(super) struct CompiledModels {
    pub(super) encoder: Arc<PhonemeEncoder>,
    pub(super) lm: LanguageModel,
    pub(super) prosody: Arc<Prosody>,
    pub(super) acoustic: Arc<Acoustic>,
    pub(super) vocoder: Vocoder,
    pub(super) style_layout: Option<StyleLayout>,
}

#[derive(Clone, Copy)]
pub struct ModelMemory([(usize, usize); 3]);

impl ModelMemory {
    pub fn bytes(&self) -> usize {
        m::sum(self.0.map(|(_, bytes)| bytes))
    }

    pub fn retained_bytes<'a>(models: impl IntoIterator<Item = &'a Self>) -> usize {
        let mut seen = BTreeSet::new();
        m::sum(
            models
                .into_iter()
                .flat_map(|model| model.0)
                .filter_map(|(allocation, bytes)| seen.insert(allocation).then_some(bytes)),
        )
    }
}

pub(crate) struct VoiceNodes<'a, 'b> {
    pub(crate) acoustic: &'b Node<'a>,
    pub(crate) intonation: &'b Node<'a>,
    pub(crate) accent: &'b Node<'a>,
    pub(crate) stylebook: &'b Node<'a>,
}

impl Synthesizer {
    pub fn compile(
        models: Models<'_, '_>,
        speaker: usize,
        offsets: Option<Offsets>,
        style_phones: Vec<String>,
        custom_speaker: Option<&[f32]>,
    ) -> Result<Self> {
        let encoder = PhonemeEncoder::compile(models.phonemes)?;
        let (acoustic, style_layout) = if models.acoustic.tag == "_ttsna0" {
            (
                Acoustic::Legacy(LegacyAcoustic::compile(models.acoustic)?),
                None,
            )
        } else {
            let pitch = PitchModel::compile(models.acoustic)?;
            let decoder = AcousticDecoder::compile(models.acoustic)?;
            let layout = StyleLayout::compile(models.acoustic, &pitch, &decoder)?;
            (Acoustic::Modern(pitch, decoder), Some(layout))
        };
        let compiled = Arc::new(CompiledModels {
            prosody: Arc::new(Prosody::compile(
                models.intonation,
                models.accent,
                encoder.format.clone(),
            )?),
            encoder: Arc::new(encoder),
            lm: LanguageModel::compile(models.language)?,
            acoustic: Arc::new(acoustic),
            vocoder: Vocoder::compile(models.vocoder)?,
            style_layout,
        });
        Self::bind_models(
            compiled,
            VoiceNodes {
                acoustic: models.acoustic,
                intonation: models.intonation,
                accent: models.accent,
                stylebook: models.stylebook,
            },
            speaker,
            offsets,
            style_phones,
            custom_speaker,
        )
    }

    pub(crate) fn rebind_models(
        &self,
        nodes: VoiceNodes<'_, '_>,
        speaker: usize,
        offsets: Option<Offsets>,
        style_phones: Vec<String>,
        custom_speaker: Option<&[f32]>,
    ) -> Result<Self> {
        Self::bind_models(
            Arc::clone(&self.models),
            nodes,
            speaker,
            offsets,
            style_phones,
            custom_speaker,
        )
    }

    fn bind_models(
        models: Arc<CompiledModels>,
        nodes: VoiceNodes<'_, '_>,
        speaker: usize,
        offsets: Option<Offsets>,
        style_phones: Vec<String>,
        custom_speaker: Option<&[f32]>,
    ) -> Result<Self> {
        if matches!(models.acoustic.as_ref(), Acoustic::Legacy(_)) && custom_speaker.is_some() {
            return Err(invalid("custom speakers require modern acoustic models"));
        }
        let custom = custom_speaker
            .map(|values| {
                let mut owners: Vec<_> = nodes.acoustic.children.iter().collect();
                owners.extend([nodes.intonation, nodes.accent]);
                split_speakers(values, &owners)
            })
            .transpose()?;
        let vector = |owner: &Node<'_>| match &custom {
            Some(v) => v
                .get(&owner.tag)
                .cloned()
                .ok_or_else(|| invalid("missing custom speaker owner")),
            None => speaker_vector(owner, speaker),
        };
        let (speaker_pitch, speaker_decoder) = match models.acoustic.as_ref() {
            Acoustic::Legacy(_) => (Vec::new(), Vec::new()),
            Acoustic::Modern(..) => {
                let find = |tag: &str| {
                    nodes
                        .acoustic
                        .children
                        .iter()
                        .find(|node| node.tag.starts_with(tag))
                        .ok_or_else(|| invalid("missing acoustic speaker owner"))
                };
                (vector(find("_ttsdf")?)?, vector(find("_ttsdt")?)?)
            }
        };
        let stylebook = match models.style_layout {
            Some(layout) => Stylebook::compile_for_model(
                nodes.stylebook,
                offsets,
                style_phones,
                layout.channels(),
            )?,
            None => Stylebook::compile(nodes.stylebook, offsets, style_phones)?,
        };
        Ok(Self {
            identity: Arc::new(()),
            models,
            stylebook,
            speaker,
            speaker_im: vector(nodes.intonation)?,
            speaker_accent: vector(nodes.accent)?,
            speaker_pitch,
            speaker_decoder,
        })
    }

    pub fn shares_models(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.models, &other.models)
    }

    pub fn model_bytes(&self) -> usize {
        self.model_memory().bytes()
    }

    pub fn model_memory(&self) -> ModelMemory {
        let models = &self.models;
        let acoustic = match models.acoustic.as_ref() {
            Acoustic::Legacy(model) => model.estimated_bytes(),
            Acoustic::Modern(pitch, decoder) => {
                m::sum([pitch.estimated_bytes(), decoder.estimated_bytes()])
            }
        };
        let local = m::arc::<CompiledModels>(m::sum([
            m::arc::<PhonemeEncoder>(models.encoder.estimated_bytes()),
            models.lm.local_bytes(),
            m::arc::<Prosody>(models.prosody.estimated_bytes()),
            m::arc::<Acoustic>(acoustic),
            models.vocoder.estimated_bytes(),
        ]));
        let [characters, transformer] = models.lm.shared_allocations();
        ModelMemory([(Arc::as_ptr(models).addr(), local), characters, transformer])
    }

    /// Retained voice-specific heap charge.
    pub fn voice_bytes(&self) -> usize {
        m::sum([
            m::arc::<()>(0),
            self.stylebook.estimated_bytes(),
            m::values(&self.speaker_im),
            m::values(&self.speaker_accent),
            m::values(&self.speaker_pitch),
            m::values(&self.speaker_decoder),
        ])
    }

    /// Retained heap charge, excluding the inline Synthesizer itself.
    pub fn estimated_bytes(&self) -> usize {
        self.voice_bytes().saturating_add(self.model_bytes())
    }
}
