//! Typed, already resolved model resources for application voice catalogs.
use super::{Manifest, bytes};
use crate::{
    Result, dnni, invalid,
    language::CharacterCache,
    stylebook::Offsets,
    synthesis::{Edits, Models, Parameters, Synthesizer},
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
        if resources.stylebook.is_empty() || resources.stylebook.len() > 16 * 1024 * 1024 {
            return Err(invalid("catalog stylebook byte limit"));
        }
        let data = [
            &resources.phonemes,
            &resources.language,
            &resources.intonation,
            &resources.accent,
            &resources.acoustic,
            &resources.vocoder,
        ]
        .into_iter()
        .map(|p| bytes(p, dnni::MAX_BYTES))
        .collect::<Result<Vec<_>>>()?;
        let roots = data
            .iter()
            .map(|b| dnni::parse(b))
            .collect::<Result<Vec<_>>>()?;
        let stylebook = dnni::parse(&resources.stylebook)?;
        let mut synth = Synthesizer::compile(
            Models {
                phonemes: &roots[0],
                language: &roots[1],
                intonation: &roots[2],
                accent: &roots[3],
                acoustic: &roots[4],
                vocoder: &roots[5],
                stylebook: &stylebook,
            },
            resources.speaker,
            resources.offsets.clone(),
            resources.style_phonemes.clone(),
            resources.speaker_vector.as_deref(),
        )?;
        synth.set_vocoder_effects(resources.vocoder_effects.clone());
        if let Some(cache) = &resources.character_cache {
            synth.set_character_cache(CharacterCache::parse(
                &bytes(&cache.descriptor, 65_536)?,
                &bytes(&cache.keys, 16 * 1024 * 1024)?,
                &bytes(&cache.data, crate::tensor::MAX_ELEMENTS * 4)?,
            )?)?;
        }
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
