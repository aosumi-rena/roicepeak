//! Catalog selection and resource refresh.
use crate::app::document::VoiceRef;
use crate::app::{catalog::Catalog, editor::EditorApp, render::Config};
use eframe::egui;
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::Duration,
};

struct LoadedCatalog {
    catalog: Catalog,
    config: Result<Option<Config>, String>,
    avatars: BTreeMap<String, Avatar>,
}

pub(super) struct CatalogLoad {
    roots: Vec<PathBuf>,
    dictionary: Option<PathBuf>,
    result: Receiver<Result<LoadedCatalog, String>>,
}

pub(super) struct Avatar {
    uri: String,
    bytes: Arc<[u8]>,
}

fn load_avatars(catalog: &Catalog) -> BTreeMap<String, Avatar> {
    catalog
        .voices
        .iter()
        .filter_map(|voice| {
            let bytes: Arc<[u8]> = Arc::from(voice.avatar.as_deref()?);
            let mut content = std::collections::hash_map::DefaultHasher::new();
            bytes.hash(&mut content);
            let uri = format!(
                "bytes://voice-avatar/{}/{:016x}.jpg",
                voice.key,
                content.finish()
            );
            Some((voice.key.clone(), Avatar { uri, bytes }))
        })
        .collect()
}

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
            .and_then(|voice| self.catalog.identity_source(&voice.collection_name))
            .map_or(key, |voice| voice.key.as_str());
        if let Some(avatar) = self.avatar_cache.get(key) {
            egui::Image::from_bytes(avatar.uri.clone(), avatar.bytes.clone())
                .corner_radius(rect.width() * 0.5)
                .paint_at(ui, rect);
        }
    }
    pub(crate) fn set_portrait_visible(&mut self, visible: bool) {
        if self.resources.show_portrait != visible {
            self.resources.show_portrait = visible;
            if let Err(error) = self.settings_state.save_now(&self.resources) {
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
            .filter(|voice| voice.collection_name == preferred || voice.name == preferred)
            .filter_map(|voice| Some((voice, voice.default_language?)))
            .reduce(|current, voice| {
                if voice.0.version > current.0.version {
                    voice
                } else {
                    current
                }
            })
            .or_else(|| {
                self.catalog
                    .voices
                    .iter()
                    .find_map(|voice| Some((voice, voice.default_language?)))
            })
            .map(|(voice, language)| VoiceRef::latest(&voice.collection_name, language.name()))
            .unwrap_or_default()
    }
    pub(crate) fn append_narrator(&self, requested: Option<&str>) -> VoiceRef {
        requested
            .and_then(|key| self.catalog.instance(key))
            .and_then(|voice| {
                Some(VoiceRef::latest(
                    &voice.collection_name,
                    voice.default_language?.name(),
                ))
            })
            .or_else(|| {
                self.project
                    .blocks
                    .last()
                    .map(|block| block.narrator.clone())
            })
            .unwrap_or_else(|| self.default_narrator())
    }
    pub(crate) fn narrator_name(&self, key: &VoiceRef) -> String {
        if let Some(voice) = self.catalog.voice(key) {
            crate::app::i18n::narrator_name(self.locale, &voice.name).to_owned()
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
            .and_then(|voice| self.catalog.identity_source(&voice.collection_name))
            .map(|voice| egui::Color32::from_rgb(voice.color[0], voice.color[1], voice.color[2]))
            .unwrap_or(crate::app::style::TEXT)
    }
    pub(crate) fn reload_voices(&mut self) -> Result<(), String> {
        if self.catalog_load.is_some() {
            return Ok(());
        }
        let roots = self.resources.voice_roots.clone();
        let dictionary = self.resources.dictionary.clone();
        let (send, result) = mpsc::sync_channel(1);
        let load_roots = roots.clone();
        let load_dictionary = dictionary.clone();
        thread::Builder::new()
            .name("roicepeak-catalog".into())
            .spawn(move || {
                let loaded = Catalog::load_for_editor(&load_roots).map(|catalog| {
                    let config = if catalog.voices.is_empty() {
                        Ok(None)
                    } else {
                        Config::from_catalog(load_dictionary, &catalog).map(Some)
                    };
                    let avatars = load_avatars(&catalog);
                    LoadedCatalog {
                        catalog,
                        config,
                        avatars,
                    }
                });
                let _ = send.send(loaded);
            })
            .map_err(|error| error.to_string())?;
        self.catalog_load = Some(CatalogLoad {
            roots,
            dictionary,
            result,
        });
        Ok(())
    }

    pub(super) fn poll_voice_catalog(&mut self, ctx: &egui::Context) {
        let Some(load) = &self.catalog_load else {
            return;
        };
        let result = match load.result.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(40));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("Voice catalog worker stopped".into()),
        };
        let current = load.roots == self.resources.voice_roots
            && load.dictionary == self.resources.dictionary;
        self.catalog_load = None;
        if !current {
            if let Err(error) = self.reload_voices() {
                self.error = Some(error);
            }
            ctx.request_repaint();
            return;
        }
        let (catalog, config, avatars) = match result {
            Ok(LoadedCatalog {
                catalog,
                config: Ok(config),
                avatars,
            }) => (catalog, config, avatars),
            Ok(LoadedCatalog {
                catalog,
                config: Err(error),
                avatars,
            }) => {
                self.error = Some(error);
                (catalog, None, avatars)
            }
            Err(error) => {
                self.error = Some(error);
                (Catalog::default(), None, Default::default())
            }
        };
        self.query_basis_current = false;
        self.invalidate_audio();
        self.portrait_state.clear_resources();
        for (key, old) in &self.avatar_cache {
            if avatars.get(key).is_none_or(|new| new.uri != old.uri) {
                ctx.forget_image(&old.uri);
            }
        }
        self.avatar_cache = avatars;
        self.catalog = catalog;
        self.render_config = config.map(|mut config| {
            config.value_policy = self.resources.value_policy();
            config
        });
        let default = self.default_narrator();
        for block in &mut self.project.blocks {
            if block.narrator.is_empty() {
                block.narrator = default.clone();
            }
        }
        self.prepare_project();
    }
}
