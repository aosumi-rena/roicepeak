//! Catalog selection and resource refresh.
use crate::app::document::VoiceRef;
use crate::app::{catalog::Catalog, editor::EditorApp, render::Config};
use eframe::egui;
use std::hash::{Hash, Hasher};

impl EditorApp {
    pub(crate) fn selected_voice(&self) -> Option<&crate::app::catalog::Voice> {
        self.catalog
            .voice(&self.project.blocks.get(self.selected()?)?.narrator)
    }
    pub(crate) fn narrator_color(&self, key: &VoiceRef) -> egui::Color32 {
        self.catalog
            .identity_source(&key.identity)
            .map(|voice| egui::Color32::from_rgb(voice.color[0], voice.color[1], voice.color[2]))
            .unwrap_or(crate::app::style::TEXT)
    }
    pub(crate) fn paint_avatar(&self, ui: &egui::Ui, reference: &VoiceRef, rect: egui::Rect) {
        if let Some(voice) = self.catalog.identity_source(&reference.identity) {
            self.paint_instance_avatar(ui, &voice.key, rect);
        }
    }
    pub(crate) fn paint_instance_avatar(&self, ui: &egui::Ui, key: &str, rect: egui::Rect) {
        let key = self
            .catalog
            .instance(key)
            .and_then(|voice| self.catalog.identity_source(&voice.name))
            .map_or(key, |voice| voice.key.as_str());
        if let Some(bytes) = self.avatar_cache.get(key) {
            let mut content = std::collections::hash_map::DefaultHasher::new();
            bytes.hash(&mut content);
            egui::Image::from_bytes(
                format!("bytes://voice-avatar/{key}/{:016x}.jpg", content.finish()),
                bytes.clone(),
            )
            .corner_radius(rect.width() * 0.5)
            .paint_at(ui, rect);
        }
    }
    pub(crate) fn set_portrait_visible(&mut self, visible: bool) {
        if self.resources.show_portrait != visible {
            self.resources.show_portrait = visible;
            if let Err(error) = self.resources.save() {
                self.error = Some(error);
            }
        }
    }
    pub(crate) fn default_narrator(&self) -> VoiceRef {
        let preferred = self
            .resources
            .preferred_voice
            .as_deref()
            .unwrap_or("彩澄しゅお");
        self.catalog
            .voices
            .iter()
            .filter(|voice| {
                voice.name == preferred
                    && voice
                        .languages
                        .iter()
                        .any(|language| language == "japanese")
            })
            .reduce(|current, voice| {
                if voice.version > current.version {
                    voice
                } else {
                    current
                }
            })
            .or_else(|| {
                self.catalog.voices.iter().find(|voice| {
                    voice
                        .languages
                        .iter()
                        .any(|language| language == "japanese")
                })
            })
            .map(|voice| VoiceRef::latest(&voice.name, "japanese"))
            .unwrap_or_default()
    }
    pub(crate) fn append_narrator(&self, requested: Option<&str>) -> VoiceRef {
        requested
            .and_then(|key| self.catalog.instance(key))
            .map(|voice| VoiceRef::latest(&voice.name, "japanese"))
            .or_else(|| {
                self.project
                    .blocks
                    .last()
                    .map(|block| block.narrator.clone())
            })
            .unwrap_or_else(|| self.default_narrator())
    }
    pub(crate) fn narrator_name(&self, key: &VoiceRef) -> String {
        if self.catalog.voice(key).is_some() {
            crate::app::i18n::narrator_name(self.locale, &key.identity).to_owned()
        } else {
            if key.is_empty() {
                self.tr("ボイスを選択", "Select a voice")
            } else {
                self.tr("利用できないボイス", "Unavailable voice")
            }
            .to_owned()
        }
    }
    pub(crate) fn instance_name(&self, key: &str) -> String {
        self.catalog
            .instance(key)
            .map(|voice| crate::app::i18n::narrator_name(self.locale, &voice.name).to_owned())
            .unwrap_or_else(|| {
                self.tr("利用できないボイス", "Unavailable voice")
                    .to_owned()
            })
    }
    pub(crate) fn instance_color(&self, key: &str) -> egui::Color32 {
        self.catalog
            .instance(key)
            .and_then(|voice| self.catalog.identity_source(&voice.name))
            .map(|voice| egui::Color32::from_rgb(voice.color[0], voice.color[1], voice.color[2]))
            .unwrap_or(crate::app::style::TEXT)
    }
    pub(crate) fn reload_voices(&mut self) -> Result<(), String> {
        let catalog = Catalog::load_roots(&self.resources.voice_roots)?;
        let config = match self.resources.dictionary.clone() {
            Some(dictionary) if !catalog.voices.is_empty() => {
                Some(Config::from_catalog(dictionary, &catalog)?)
            }
            _ => None,
        };
        self.replace_voice_catalog(catalog, config);
        Ok(())
    }
    pub(crate) fn replace_voice_catalog(&mut self, catalog: Catalog, config: Option<Config>) {
        self.query_basis_current = false;
        self.invalidate_audio();
        self.portrait_state.clear_resources();
        self.avatar_cache = catalog
            .voices
            .iter()
            .filter_map(|voice| {
                voice
                    .avatar
                    .as_ref()
                    .map(|bytes| (voice.key.clone(), std::sync::Arc::from(bytes.clone())))
            })
            .collect();
        self.catalog = catalog;
        self.render_config = config.map(|mut config| {
            config.value_policy = self.resources.value_policy();
            config
        });
        let default = self.default_narrator();
        for block in &mut self.project.blocks {
            if block.narrator.is_empty() && block.text.is_empty() {
                block.narrator = default.clone();
            }
        }
        self.prepare_project();
    }
}
