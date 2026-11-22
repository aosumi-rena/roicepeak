//! Typed, already resolved model resources for application voice catalogs.
use super::{Manifest, bytes};
use crate::{
    Result, dnni, invalid,
    language::CharacterCache,
    stylebook::Offsets,
    synthesis::{Edits, Models, Parameters, Synthesizer, VoiceNodes},
    vocoder::Effects,
};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct CharacterCacheResources {
    pub descriptor: PathBuf,
    pub keys: PathBuf,
    pub data: PathBuf,
}
impl CharacterCacheResources {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            crate::memory::path(&self.descriptor),
            crate::memory::path(&self.keys),
            crate::memory::path(&self.data),
        ])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelResources {
    pub phonemes: PathBuf,
    pub language: PathBuf,
    pub intonation: PathBuf,
    pub accent: PathBuf,
    pub acoustic: PathBuf,
    pub vocoder: PathBuf,
    pub stylebook: Vec<u8>,
    pub speaker_vector: Option<Vec<f32>>,
    pub speaker: usize,
    pub offsets: Option<Offsets>,
    pub style_phonemes: Vec<String>,
    pub character_cache: Option<CharacterCacheResources>,
    pub weights: Vec<f32>,
    pub fixed_seeding: bool,
    pub invert_intonation: bool,
    pub vocoder_effects: Effects,
}

impl ModelResources {
    /// Retained heap charge, excluding the inline ModelResources itself.
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        m::sum([
            m::path(&self.phonemes),
            m::path(&self.language),
            m::path(&self.intonation),
            m::path(&self.accent),
            m::path(&self.acoustic),
            m::path(&self.vocoder),
            m::values(&self.stylebook),
            m::optional(&self.speaker_vector, m::values),
            m::nested(&self.style_phonemes, m::string),
            m::optional(
                &self.character_cache,
                CharacterCacheResources::estimated_bytes,
            ),
            m::values(&self.weights),
            self.vocoder_effects.estimated_bytes(),
        ])
    }

    pub fn file_paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![
            self.phonemes.clone(),
            self.language.clone(),
            self.intonation.clone(),
            self.accent.clone(),
            self.acoustic.clone(),
            self.vocoder.clone(),
        ];
        if let Some(cache) = &self.character_cache {
            paths.extend([
                cache.descriptor.clone(),
                cache.keys.clone(),
                cache.data.clone(),
            ]);
        }
        paths
    }
}

impl Manifest {
    pub fn from_resources(resources: &ModelResources) -> Result<Self> {
        Self::load_resources(resources, None)
    }

    /// Rebind a voice using a manifest built from the same validated model files and effects.
    pub fn from_resources_reusing(resources: &ModelResources, source: &Self) -> Result<Self> {
        Self::load_resources(resources, Some(source))
    }

    fn load_resources(resources: &ModelResources, source: Option<&Self>) -> Result<Self> {
        if resources.stylebook.is_empty() || resources.stylebook.len() > 16 * 1024 * 1024 {
            return Err(invalid("catalog stylebook byte limit"));
        }
        let intonation = bytes(&resources.intonation, dnni::MAX_BYTES)?;
        let accent = bytes(&resources.accent, dnni::MAX_BYTES)?;
        let acoustic = bytes(&resources.acoustic, dnni::MAX_BYTES)?;
        let intonation = dnni::parse(&intonation)?;
        let accent = dnni::parse(&accent)?;
        let acoustic = dnni::parse(&acoustic)?;
        let stylebook = dnni::parse(&resources.stylebook)?;
        let offsets = resources.offsets.clone();
        let phones = resources.style_phonemes.clone();
        let speaker = resources.speaker;
        let vector = resources.speaker_vector.as_deref();
        let synth = if let Some(source) = source {
            source.synth.rebind_models(
                VoiceNodes {
                    acoustic: &acoustic,
                    intonation: &intonation,
                    accent: &accent,
                    stylebook: &stylebook,
                },
                speaker,
                offsets,
                phones,
                vector,
            )?
        } else {
            let phonemes = bytes(&resources.phonemes, dnni::MAX_BYTES)?;
            let language = bytes(&resources.language, dnni::MAX_BYTES)?;
            let vocoder = bytes(&resources.vocoder, dnni::MAX_BYTES)?;
            let mut synth = Synthesizer::compile(
                Models {
                    phonemes: &dnni::parse(&phonemes)?,
                    language: &dnni::parse(&language)?,
                    intonation: &intonation,
                    accent: &accent,
                    acoustic: &acoustic,
                    vocoder: &dnni::parse(&vocoder)?,
                    stylebook: &stylebook,
                },
                speaker,
                offsets,
                phones,
                vector,
            )?;
            synth.set_vocoder_effects(resources.vocoder_effects.clone());
            if let Some(cache) = &resources.character_cache {
                synth.set_character_cache(CharacterCache::parse(
                    &bytes(&cache.descriptor, 65_536)?,
                    &bytes(&cache.keys, 16 * 1024 * 1024)?,
                    &bytes(&cache.data, crate::tensor::MAX_ELEMENTS * 4)?,
                )?)?;
            }
            synth
        };
        if resources.weights.len() != synth.emotions().len()
            || resources.weights.iter().any(|v| !v.is_finite())
        {
            return Err(invalid("catalog baseline emotion weights"));
        }
        Ok(Self {
            synth,
            weights: resources.weights.clone(),
            parameters: Parameters {
                fixed_seeding: resources.fixed_seeding,
                invert_intonation: resources.invert_intonation,
                ..Parameters::default()
            },
            edits: Edits::default(),
        })
    }
}
