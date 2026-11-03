//! Identity-wide presentation metadata.
use super::{Catalog, Voice};
use crate::app::render::EmotionBinding;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct EmotionMetadata {
    pub key: String,
    pub label: String,
    pub is_main: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct EmotionControl {
    pub binding: EmotionBinding,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct IdentityMetadata {
    pub primary_source: usize,
    pub vendor: String,
    pub languages: Vec<String>,
    pub emotions: Vec<EmotionMetadata>,
    pub controls: Vec<EmotionControl>,
    pub preset_controls: Vec<EmotionBinding>,
}

impl Catalog {
    pub fn identity_metadata(&self, identity: &str) -> Option<&IdentityMetadata> {
        self.identities.get(identity)
    }

    pub fn identity_source(&self, identity: &str) -> Option<&Voice> {
        self.voices
            .get(self.identity_metadata(identity)?.primary_source)
    }

    pub(super) fn rebuild_identities(&mut self) {
        let mut order = Vec::new();
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, voice) in self.voices.iter().enumerate() {
            if !groups.contains_key(&voice.name) {
                order.push(voice.name.clone());
            }
            groups.entry(voice.name.clone()).or_default().push(index);
        }
        self.identities.clear();
        for identity in &order {
            let sources = groups.get_mut(identity).unwrap();
            // Retain configured discovery order for equal versions.
            sources.sort_by(|a, b| self.voices[*b].version.cmp(&self.voices[*a].version));
            let mut metadata = IdentityMetadata {
                primary_source: sources[0],
                vendor: self.voices[*sources.last().unwrap()].vendor.clone(),
                languages: Vec::new(),
                emotions: Vec::new(),
                controls: Vec::new(),
                preset_controls: Vec::new(),
            };
            let mut languages = BTreeSet::new();
            let mut emotions = BTreeSet::new();
            for &index in sources.iter() {
                let source = &self.voices[index];
                for language in &source.languages {
                    if languages.insert(language.clone()) {
                        metadata.languages.push(language.clone());
                    }
                }
                for emotion in &source.emotions {
                    if emotions.insert(emotion.key.clone()) {
                        metadata.emotions.push(emotion.clone());
                    }
                }
            }
            self.identities.insert(identity.clone(), metadata);
        }
        let mut labels = BTreeMap::new();
        let mut nonmain = BTreeSet::new();
        for identity in &order {
            for emotion in &self.identities[identity].emotions {
                labels
                    .entry(emotion.key.clone())
                    .or_insert_with(|| emotion.label.clone());
                if !emotion.is_main {
                    nonmain.insert(emotion.key.clone());
                }
            }
        }
        for metadata in self.identities.values_mut() {
            for emotion in &metadata.emotions {
                let binding = EmotionBinding {
                    key: emotion.key.clone(),
                    label: labels[&emotion.key].clone(),
                };
                if nonmain.contains(&emotion.key) {
                    metadata.controls.push(EmotionControl {
                        binding: binding.clone(),
                        enabled: emotion.enabled,
                    });
                }
                if !emotion.is_main {
                    metadata.preset_controls.push(binding);
                }
            }
        }
    }
}
