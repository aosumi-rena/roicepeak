//! Runtime bindings from voice catalog metadata.
use crate::app::document::{VoiceRef, VoiceSourceInfo, resolve_source};
use roicepeak::manifest::ModelResources;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub value_policy: roicepeak::scalar_layers::ValuePolicy,
    pub dictionary: PathBuf,
    pub voices: BTreeMap<String, VoiceBinding>,
    pub selections: Vec<VoiceSource>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceSource {
    pub identity: String,
    pub languages: Vec<String>,
    pub version: u32,
    pub binding_key: String,
}
impl VoiceSourceInfo for VoiceSource {
    fn identity(&self) -> &str {
        &self.identity
    }
    fn languages(&self) -> &[String] {
        &self.languages
    }
    fn version(&self) -> u32 {
        self.version
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceBinding {
    pub source: PathBuf,
    pub source_bytes: Arc<Vec<u8>>,
    pub metadata_files: Vec<(PathBuf, Vec<u8>)>,
    pub resources: ModelResources,
    pub main_emotion: Option<String>,
    /// Metadata UI order and declared engine keys. Unmatched keys remain UI
    /// controls without contributing synthesis weights.
    pub emotion_bindings: Vec<EmotionBinding>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct EmotionBinding {
    pub label: String,
    pub key: String,
}
impl Config {
    pub fn from_catalog(
        dictionary: PathBuf,
        catalog: &crate::app::catalog::Catalog,
    ) -> Result<Self, String> {
        let config = Self {
            value_policy: Default::default(),
            dictionary,
            voices: catalog
                .voices
                .iter()
                .map(|voice| (voice.key.clone(), voice.binding.clone()))
                .collect(),
            selections: catalog
                .voices
                .iter()
                .map(|voice| VoiceSource {
                    identity: voice.name.clone(),
                    languages: voice.languages.clone(),
                    version: voice.version,
                    binding_key: voice.key.clone(),
                })
                .collect(),
        };
        config.validate()?;
        Ok(config)
    }
    pub fn binding(&self, reference: &VoiceRef) -> Result<&VoiceBinding, String> {
        if reference.language != "japanese" {
            return Err(format!(
                "No text frontend for language {:?}",
                reference.language
            ));
        }
        let selected = resolve_source(reference, &self.selections)
            .ok_or_else(|| format!("Unavailable narrator {reference}"))?;
        self.voices
            .get(&selected.binding_key)
            .ok_or_else(|| format!("No model binding for narrator {reference}"))
    }
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.dictionary.as_os_str().is_empty()
            || self.voices.is_empty()
            || self.voices.len() > 256
            || self.selections.len() != self.voices.len()
        {
            return Err("A dictionary and 1 to 256 resolved catalog voices are required".into());
        }
        for source in &self.selections {
            if source.identity.is_empty()
                || source.identity.len() > 4096
                || source.languages.is_empty()
                || !self.voices.contains_key(&source.binding_key)
            {
                return Err("Invalid narrator selection metadata".into());
            }
        }
        for (name, voice) in &self.voices {
            if name.is_empty()
                || name.len() > 4096
                || voice.source.as_os_str().is_empty()
                || voice.source_bytes.is_empty()
                || voice.source_bytes.len() > 8 * 1024 * 1024
                || voice.emotion_bindings.len() > 1024
            {
                return Err("Invalid narrator/resource binding".into());
            }
            let mut labels = BTreeSet::new();
            let mut keys = BTreeSet::new();
            for EmotionBinding { label, key } in &voice.emotion_bindings {
                if label.is_empty() || key.is_empty() || label.len() > 4096 || key.len() > 4096 {
                    return Err("Invalid emotion label binding".into());
                }
                if !labels.insert(label)
                    || !keys.insert(key)
                    || voice.main_emotion.as_ref() == Some(key)
                {
                    return Err("Emotion controls require unique labels and engine keys distinct from the main emotion".into());
                }
            }
        }
        Ok(())
    }
}
