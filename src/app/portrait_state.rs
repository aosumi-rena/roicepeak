//! Transient portrait preferences and bounded lazy artwork cache.
use crate::app::{
    catalog::{IdentityMetadata, Portrait, Voice},
    document::{Block, Parameters},
};
use eframe::egui;
use std::collections::BTreeMap;
const MAX_BLOCK_CHOICES: usize = 10_000;
const MAX_NARRATOR_CHOICES: usize = 256;
const MAX_IMAGES: usize = 16;
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_DECODED_BYTES: u64 = 128 * 1024 * 1024;
fn decoded_bytes(image: &Portrait) -> u64 {
    u64::from(image.pixel_size[0]) * u64::from(image.pixel_size[1]) * 4
}
#[derive(Clone, Copy, Default)]
enum Scope {
    #[default]
    Block,
    Narrator,
}
struct Entry {
    source: u64,
    emotion: String,
    portrait: Option<Portrait>,
    uri: Option<String>,
    problem: Option<String>,
}
pub(super) struct Loaded {
    pub(super) portrait: Portrait,
    pub(super) uri: String,
}
#[derive(Default)]
pub struct State {
    pub(super) views: super::view::State,
    block_choices: BTreeMap<(u64, String), String>,
    narrator_choices: BTreeMap<String, String>,
    scope: Scope,
    images: Vec<Entry>,
    reset_images: bool,
    pub last_problem: Option<String>,
}
impl State {
    /// Clear cached block IDs when replacing a document or restoring history.
    pub fn clear_choices(&mut self) {
        self.block_choices.clear();
        self.narrator_choices.clear();
        self.scope = Scope::Block;
    }
    /// Image-loader cleanup is deferred to the next paint, where a Context exists.
    pub fn clear_resources(&mut self) {
        self.reset_images = true;
        self.clear_choices();
        self.last_problem = None;
    }
    pub fn note_block_change(&mut self, block: u64, narrator: &str, key: &str) {
        let address = (block, narrator.to_owned());
        if !self.block_choices.contains_key(&address)
            && self.block_choices.len() == MAX_BLOCK_CHOICES
        {
            self.block_choices.pop_first();
        }
        self.block_choices.insert(address, key.to_owned());
        self.scope = Scope::Block;
    }
    pub fn note_narrator_change(&mut self, narrator: &str, key: &str) {
        if !self.narrator_choices.contains_key(narrator)
            && self.narrator_choices.len() == MAX_NARRATOR_CHOICES
        {
            self.narrator_choices.pop_first();
        }
        self.narrator_choices
            .insert(narrator.to_owned(), key.to_owned());
        self.scope = Scope::Narrator;
    }
    pub(super) fn selected_key(
        &self,
        block: &Block,
        voice: &Voice,
        metadata: &IdentityMetadata,
        corrections: Option<&Parameters>,
    ) -> String {
        let main = voice.binding.main_emotion.as_deref().unwrap_or("");
        let mut weights = BTreeMap::from([(main.to_owned(), 0.0_f32)]);
        for control in &metadata.emotions {
            let value = block
                .parameters
                .emotions
                .get(&control.key)
                .copied()
                .unwrap_or_default()
                .normalized();
            let correction = corrections
                .and_then(|p| p.emotions.get(&control.key))
                .copied()
                .unwrap_or_default()
                .normalized();
            weights.insert(control.key.clone(), (value + correction).clamp(0.0, 1.0));
        }
        let preferred = match self.scope {
            Scope::Block => self.block_choices.get(&(block.id, voice.name.clone())),
            Scope::Narrator => self.narrator_choices.get(&voice.name),
        };
        let mut selected = preferred
            .filter(|key| weights.contains_key(*key))
            .map_or(main, String::as_str)
            .to_owned();
        let mut maximum = weights.get(&selected).copied().unwrap_or(0.0);
        // Keep the first maximum; portrait weights start at zero.
        for (key, value) in weights {
            if value > maximum {
                maximum = value;
                selected = key;
            }
        }
        if maximum == 0.0 {
            main.to_owned()
        } else {
            selected
        }
    }
    fn forget(entry: &Entry, ctx: &egui::Context) {
        if let Some(uri) = &entry.uri {
            ctx.forget_image(uri);
        }
    }
    pub(super) fn image(
        &mut self,
        ctx: &egui::Context,
        voice: &Voice,
        emotion: &str,
    ) -> Option<Loaded> {
        if self.reset_images {
            for entry in &self.images {
                Self::forget(entry, ctx);
            }
            self.images.clear();
            self.reset_images = false;
        }
        let source = voice.portrait_sources.identity();
        if let Some(entry) = self
            .images
            .iter()
            .find(|entry| entry.source == source && entry.emotion == emotion)
        {
            self.last_problem = entry.problem.clone();
            return entry
                .portrait
                .clone()
                .zip(entry.uri.clone())
                .map(|(portrait, uri)| Loaded { portrait, uri });
        }
        let result = if voice.binding.main_emotion.as_deref() == Some(emotion) {
            voice.portrait_main.clone().map_or_else(
                || voice.portrait_sources.load(emotion),
                |image| Ok(Some(image)),
            )
        } else {
            voice.portrait_sources.load(emotion)
        };
        let (mut portrait, mut problem) = match result {
            Ok(image) => (image, None),
            Err(error) => (None, Some(error)),
        };
        if portrait
            .as_ref()
            .is_some_and(|image| decoded_bytes(image) > MAX_DECODED_BYTES)
        {
            portrait = None;
            problem = Some("Portrait exceeds decoded image cache budget".into());
        }
        let bytes = portrait.as_ref().map_or(0, |image| image.bytes.len());
        let decoded = portrait.as_ref().map_or(0, decoded_bytes);
        while self.images.len() >= MAX_IMAGES
            || self
                .images
                .iter()
                .filter_map(|entry| entry.portrait.as_ref())
                .map(|image| image.bytes.len())
                .sum::<usize>()
                + bytes
                > MAX_IMAGE_BYTES
            || self
                .images
                .iter()
                .filter_map(|entry| entry.portrait.as_ref())
                .map(decoded_bytes)
                .sum::<u64>()
                + decoded
                > MAX_DECODED_BYTES
        {
            Self::forget(&self.images.remove(0), ctx);
        }
        let uri = portrait.as_ref().map(|image| {
            format!(
                "bytes://standing-portrait/{}/{source:016x}/{:016x}/{emotion}.jpg",
                voice.key, image.content_key
            )
        });
        let loaded = portrait
            .clone()
            .zip(uri.clone())
            .map(|(portrait, uri)| Loaded { portrait, uri });
        self.last_problem = problem.clone();
        self.images.push(Entry {
            source,
            emotion: emotion.to_owned(),
            portrait,
            uri,
            problem,
        });
        loaded
    }
}
