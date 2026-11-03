//! Format-1 plain voice descriptions and narrator metadata.
#[path = "factory.rs"]
mod factory;
use super::{Voice, resolve};
use crate::app::render::{EmotionBinding, VoiceBinding};
use roicepeak::{
    dnni,
    manifest::{CharacterCacheResources, ModelResources},
    stylebook::{Offsets, Stylebook},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Deserialize)]
struct Description {
    name: String,
    format: u32,
    base: Option<serde_json::Value>,
    network: String,
    components: Components,
    pack_names: PackNames,
    entrance: Entrance,
}
#[derive(Deserialize)]
struct Components {
    vo: String,
    lm: String,
    charembeds: Option<String>,
}
#[derive(Deserialize)]
struct PackNames {
    entrance: String,
    tts: Option<String>,
    vo: Option<String>,
}
#[derive(Deserialize)]
struct Entrance {
    files: BTreeMap<String, String>,
    #[serde(default)]
    assets: BTreeMap<String, String>,
}
#[derive(Deserialize)]
struct Narrator {
    inst_id: String,
    narrator_id: String,
    version: u32,
    vendor: String,
    supported_language_list: Vec<String>,
    can_commercial: bool,
    #[serde(default)]
    trial_version: bool,
    key_color: String,
    main_emotion_engine_key: String,
    emotion_list: Vec<Emotion>,
    #[serde(default)]
    standing_pic_default_region: Option<[i32; 4]>,
}
#[derive(Deserialize)]
struct Emotion {
    engine_key: String,
    ui_name: String,
    #[serde(default)]
    standing_pic_id: Option<String>,
}

fn text(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 4096 || value.contains('\0') {
        Err("Invalid narrator metadata text".into())
    } else {
        Ok(())
    }
}
fn inline(entrance: &Entrance, key: &str, limit: usize) -> Result<Option<Vec<u8>>, String> {
    entrance
        .files
        .get(key)
        .map(|v| resolve::decode(v, limit))
        .transpose()
}
fn required(entrance: &Entrance, key: &str, limit: usize) -> Result<Vec<u8>, String> {
    inline(entrance, key, limit)?.ok_or_else(|| format!("Voice entrance lacks {key}"))
}
/// Artwork can be inline bytes or a content-addressed JPEG in the resource root.
/// Missing artwork does not disable synthesis.
fn artwork(root: &resolve::Root, entrance: &Entrance, key: &str, limit: usize) -> Option<Vec<u8>> {
    match super::artwork::load_declared(root, &entrance.files, &entrance.assets, key, limit) {
        Ok(bytes) => bytes,
        Err(error) => {
            root.problem(format!("Optional artwork {key:?}: {error}"));
            None
        }
    }
}

fn public_artwork_key(value: &str) -> Result<String, String> {
    text(value)?;
    resolve::relative(Path::new(value))?;
    if value.contains('\\') {
        return Err("Public artwork identity must use relative forward-slash components".into());
    }
    Ok(format!("pub/{value}"))
}

fn offset(entrance: &Entrance, key: &str) -> Result<Option<usize>, String> {
    inline(entrance, key, 4)?
        .map(|b| {
            let word: [u8; 4] = b
                .try_into()
                .map_err(|_| "Stylebook offset must contain uint32")?;
            Ok(u32::from_le_bytes(word) as usize)
        })
        .transpose()
        .map(|v| v.filter(|n| *n != 0))
}

pub(super) fn load(root: &resolve::Root, source: &Path, bytes: Vec<u8>) -> Result<Voice, String> {
    let canonical_root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    if !source.starts_with(&canonical_root) {
        return Err("Voice metadata escapes its root".into());
    }
    let description: Description = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    text(&description.name)?;
    if description.format != 1 || description.base.is_some() {
        return Err(
            "A complete format-1 plain voice is required; inherited voices are not resolved".into(),
        );
    }
    if description.entrance.files.len() > 1024 || description.entrance.assets.len() > 1024 {
        return Err("Voice entrance item count limit".into());
    }
    let entrance = &description.entrance;
    let narrator: Narrator =
        serde_json::from_slice(&required(entrance, "pub/rc/idc.json", 65_536)?)
            .map_err(|e| format!("Narrator metadata: {e}"))?;
    for value in [
        &narrator.inst_id,
        &narrator.narrator_id,
        &narrator.vendor,
        &narrator.main_emotion_engine_key,
    ] {
        text(value)?;
    }
    if narrator.inst_id != description.pack_names.entrance {
        return Err("Narrator entrance identity differs from its pack declaration".into());
    }
    if narrator.supported_language_list.is_empty() || narrator.supported_language_list.len() > 16 {
        return Err("Narrator language count limit".into());
    }
    for language in &narrator.supported_language_list {
        text(language)?;
    }
    if narrator.key_color.len() != 6 || !narrator.key_color.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Narrator key color must contain six hexadecimal digits".into());
    }
    let mut color = [0u8; 3];
    for (index, channel) in color.iter_mut().enumerate() {
        *channel = u8::from_str_radix(&narrator.key_color[index * 2..index * 2 + 2], 16)
            .map_err(|e| e.to_string())?;
    }
    let network = PathBuf::from("networks").join(resolve::id(&description.network)?);
    let language = PathBuf::from("components/lm").join(resolve::id(&description.components.lm)?);
    let vocoder = PathBuf::from("components/vo").join(resolve::id(&description.components.vo)?);
    let stylebook = required(entrance, "stylebook.dnni", 16 * 1024 * 1024)?;
    let offsets = Offsets {
        aperiodicity: offset(entrance, "stylebook_bap_offset_start")?,
        volume: offset(entrance, "stylebook_volume_start")?,
        intonation: offset(entrance, "stylebook_int_offset_start")?,
        speed: offset(entrance, "stylebook_overall_speed_start")?,
        tension: if factory::is_protocol3(&description)? {
            offset(entrance, "stylebook_tension_start")?
        } else {
            None
        },
    };
    let style_phonemes = inline(entrance, "stylebook_phoneme_list", 65_536)?
        .map(|b| {
            b.split(|v| *v == 0)
                .filter(|v| !v.is_empty())
                .map(|v| {
                    std::str::from_utf8(v)
                        .map(str::to_owned)
                        .map_err(|_| "Style phonemes must be UTF-8".to_owned())
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    let style = Stylebook::compile(
        &dnni::parse(&stylebook).map_err(|e| e.to_string())?,
        Some(offsets.clone()),
        style_phonemes.clone(),
    )
    .map_err(|e| e.to_string())?;
    let main = style
        .emotions
        .iter()
        .position(|key| key == &narrator.main_emotion_engine_key)
        .ok_or("Narrator main emotion is absent from its stylebook")?;
    if narrator.emotion_list.is_empty() || narrator.emotion_list.len() > 1024 {
        return Err("Narrator emotion count limit".into());
    }
    let mut labels = BTreeSet::new();
    let mut keys = BTreeSet::new();
    // Retain declarations only. Nonmain images are resolved lazily when selected.
    let mut portrait_declarations = BTreeMap::new();
    for emotion in &narrator.emotion_list {
        if let Some(id) = emotion
            .standing_pic_id
            .as_deref()
            .filter(|id| !id.is_empty())
        {
            match public_artwork_key(id) {
                Ok(key) => {
                    portrait_declarations.insert(emotion.engine_key.clone(), key);
                }
                Err(error) => root.problem(format!("Optional portrait declaration: {error}")),
            }
        }
    }
    let portrait_bytes = portrait_declarations
        .get(&narrator.main_emotion_engine_key)
        .and_then(|key| artwork(root, entrance, key, 16 * 1024 * 1024));
    let mut emotion_bindings = Vec::new();
    let mut emotions = Vec::new();
    for emotion in narrator.emotion_list {
        text(&emotion.engine_key)?;
        text(&emotion.ui_name)?;
        if !labels.insert(emotion.ui_name.clone()) || !keys.insert(emotion.engine_key.clone()) {
            return Err("Narrator emotion label or engine key is repeated".into());
        }
        emotions.push(super::EmotionMetadata {
            key: emotion.engine_key.clone(),
            label: emotion.ui_name.clone(),
            is_main: emotion.engine_key == narrator.main_emotion_engine_key,
            enabled: !narrator.trial_version,
        });
        if emotion.engine_key != narrator.main_emotion_engine_key {
            emotion_bindings.push(EmotionBinding {
                label: emotion.ui_name,
                key: emotion.engine_key,
            });
        }
    }
    if !keys.contains(&narrator.main_emotion_engine_key) {
        return Err("Narrator main emotion has no UI declaration".into());
    }
    let mut weights = vec![0.0; style.emotions.len()];
    weights[main] = 1.0;
    let speaker_vector = inline(entrance, "spkvec", 4 * 1024 * 1024)?
        .map(|b| {
            if b.is_empty() || b.len() % 4 != 0 {
                return Err("Speaker vector must contain float32 values".to_owned());
            }
            let values = b
                .chunks_exact(4)
                .map(|w| f32::from_le_bytes(w.try_into().unwrap()))
                .collect::<Vec<_>>();
            if values.iter().any(|v| !v.is_finite()) {
                return Err("Nonfinite speaker vector".into());
            }
            Ok(values)
        })
        .transpose()?;
    let speaker = factory::speaker_identity(&description, speaker_vector.is_some())?;
    let mut metadata_files = Vec::new();
    let mut model_flag = |name: &str| -> Result<bool, String> {
        let value = resolve::flag(root, &network.join(name))?;
        let flag = value.as_ref().is_some_and(|(_, b)| b[0] == 1);
        if let Some(value) = value {
            metadata_files.push(value);
        }
        Ok(flag)
    };
    let fixed_seeding = model_flag("dds_fix_seeding")?;
    let invert_intonation = model_flag("invert_intonation")?;
    let character_cache = description
        .components
        .charembeds
        .as_deref()
        .map(|id| {
            let directory = PathBuf::from("components/charembeds").join(resolve::id(id)?);
            Ok::<_, String>(CharacterCacheResources {
                descriptor: resolve::file(root, &directory.join("desc"))?,
                keys: resolve::file(root, &directory.join("key"))?,
                data: resolve::file(root, &directory.join("data"))?,
            })
        })
        .transpose()?;
    let resources = ModelResources {
        phonemes: resolve::file(root, &network.join("pe.dnni"))?,
        language: resolve::file(root, &language.join("lm.dnni"))?,
        intonation: resolve::file(root, &network.join("im.dnni"))?,
        accent: resolve::file(root, &network.join("acc2int.dnni"))?,
        acoustic: resolve::file(root, &network.join("am.dnni"))?,
        vocoder: resolve::file(root, &vocoder.join("vocoder.dnni"))?,
        stylebook,
        speaker_vector,
        speaker,
        offsets: Some(offsets),
        style_phonemes,
        character_cache,
        weights,
        fixed_seeding,
        invert_intonation,
        vocoder_effects: factory::vocoder_effects(&description)?,
    };
    let mut model_bytes = 0u64;
    for path in resources.file_paths() {
        model_bytes = model_bytes
            .checked_add(
                fs::metadata(&path)
                    .map_err(|error| error.to_string())?
                    .len(),
            )
            .ok_or("Model byte size overflow")?;
    }
    if model_bytes > 768 * 1024 * 1024 {
        return Err("Narrator model resource byte limit".into());
    }
    let avatar = artwork(root, entrance, "pub/rc/avatar.jpg", 2 * 1024 * 1024);
    let portrait_main = match portrait_bytes
        .map(|bytes| super::artwork::portrait(bytes, narrator.standing_pic_default_region))
        .transpose()
    {
        Ok(portrait) => portrait,
        Err(error) => {
            root.problem(format!(
                "Declared standing artwork cannot be decoded: {error}"
            ));
            None
        }
    };
    let source_bytes = Arc::new(bytes);
    let portrait_sources = super::PortraitSources::new(
        root,
        Arc::clone(&source_bytes),
        portrait_declarations,
        narrator.standing_pic_default_region,
    );
    Ok(Voice {
        key: narrator.inst_id,
        name: narrator.narrator_id,
        collection_name: description.name,
        version: narrator.version,
        vendor: narrator.vendor,
        languages: narrator.supported_language_list,
        emotions,
        can_commercial: narrator.can_commercial,
        color,
        avatar,
        portrait_main,
        portrait_sources,
        binding: VoiceBinding {
            source: source.to_path_buf(),
            source_bytes,
            metadata_files,
            resources,
            main_emotion: Some(narrator.main_emotion_engine_key),
            emotion_bindings,
        },
    })
}
