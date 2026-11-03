//! Staged typed native data; conversion into editor units is a separate transaction.
#[path = "native_adapter_fields.rs"]
pub(crate) mod fields;
#[path = "native_adapter_manual.rs"]
mod manual;
use crate::app::{
    document::{OffsetMode, VoiceRef, VoiceVersion, WordState},
    native_project::NativeProject,
};
use fields::{Field, child_path, get};
pub(super) use manual::NativeSentenceDraft;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct NormalizedF32(pub u32);
impl NormalizedF32 {
    pub fn value(self) -> f32 {
        f32::from_bits(self.0)
    }
    pub fn from_authored_percent(value: f32) -> Result<Self, String> {
        let normalized = value / 100.0;
        if !value.is_finite() || !normalized.is_finite() {
            return Err("Native control must remain finite".into());
        }
        Ok(Self(normalized.to_bits()))
    }
    pub fn display_percent(self) -> Result<f32, String> {
        let display = self.value() * 100.0;
        display
            .is_finite()
            .then_some(display)
            .ok_or("Native control has no finite percent display".into())
    }
}

#[derive(Clone, Debug)]
pub(super) struct NativeParameters {
    pub scalars: [NormalizedF32; 4],
}
pub(super) fn parameters(value: &Value, path: &str) -> Result<NativeParameters, String> {
    let object = Field { value, path }.object()?;
    let mut scalars = [NormalizedF32(0); 4];
    for (index, name) in ["speed", "pitch", "pause", "volume"].iter().enumerate() {
        let p = child_path(path, name);
        scalars[index] = NormalizedF32(
            Field {
                value: get(object, name, path)?,
                path: &p,
            }
            .float32()?
            .to_bits(),
        );
    }
    Ok(NativeParameters { scalars })
}
pub(super) fn emotions(
    value: &Value,
    path: &str,
) -> Result<BTreeMap<String, NormalizedF32>, String> {
    let object = Field { value, path }.object()?;
    if object.len() > 1024 {
        return Err(format!("{path}: emotion count limit"));
    }
    object
        .iter()
        .map(|(key, value)| {
            if key.len() > 4096 || key.contains('\0') {
                return Err(format!("{path}: invalid emotion key"));
            }
            let p = child_path(path, key);
            Ok((
                key.clone(),
                NormalizedF32(Field { value, path: &p }.float32()?.to_bits()),
            ))
        })
        .collect()
}

#[derive(Clone, Debug)]
pub(super) struct NativeBlockDraft {
    pub voice: VoiceRef,
    pub offset_mode: OffsetMode,
    pub offset_seconds: f64,
    pub parameters: NativeParameters,
    pub emotions: BTreeMap<String, NormalizedF32>,
    sentences: Vec<NativeSentenceDraft>,
    pub sentence_ranges: Vec<[i32; 2]>,
}
#[derive(Clone, Debug)]
pub(super) struct NativeDocumentDraft {
    pub global: NativeParameters,
    blocks: Vec<NativeBlockDraft>,
    root: Value,
    native: NativeProject,
}
impl NativeDocumentDraft {
    pub fn blocks(&self) -> &[NativeBlockDraft] {
        &self.blocks
    }
    pub fn decode(native: &NativeProject) -> Result<Self, String> {
        let root = native.root();
        let project_value = &root["project"];
        let project = Field {
            value: project_value,
            path: "/project",
        }
        .object()?;
        let global = parameters(get(project, "params", "/project")?, "/project/params")?;
        let block_values = Field {
            value: get(project, "blocks", "/project")?,
            path: "/project/blocks",
        }
        .array(10000)?;
        let mut blocks = Vec::with_capacity(block_values.len());
        for (index, value) in block_values.iter().enumerate() {
            let path = format!("/project/blocks/{index}");
            let block = Field { value, path: &path }.object()?;
            let narrator_path = child_path(&path, "narrator");
            let narrator = Field {
                value: get(block, "narrator", &path)?,
                path: &narrator_path,
            }
            .object()?;
            let p = child_path(&narrator_path, "key");
            let identity = Field {
                value: get(narrator, "key", &narrator_path)?,
                path: &p,
            }
            .string(4096)?
            .to_owned();
            if identity.is_empty() {
                return Err(format!("{p}: empty narrator identity"));
            }
            let p = child_path(&narrator_path, "language");
            let language = Field {
                value: get(narrator, "language", &narrator_path)?,
                path: &p,
            }
            .string(128)?
            .to_owned();
            let p = child_path(&narrator_path, "narrator-version");
            let version = match (Field {
                value: get(narrator, "narrator-version", &narrator_path)?,
                path: &p,
            })
            .signed()?
            {
                -1 => VoiceVersion::Latest,
                n if n >= 0 => VoiceVersion::Exact(n as u32),
                _ => return Err(format!("{p}: unsupported narrator version")),
            };
            let voice = VoiceRef {
                identity,
                language,
                version,
            };
            if !voice.valid() {
                return Err(format!("{narrator_path}: invalid voice selector"));
            }
            let p = child_path(&path, "time-offset-mode");
            let offset_mode = match (Field {
                value: get(block, "time-offset-mode", &path)?,
                path: &p,
            })
            .signed()?
            {
                0 => OffsetMode::Absolute,
                1 => OffsetMode::PreviousStart,
                2 => OffsetMode::PreviousEnd,
                _ => return Err(format!("{p}: unsupported placement mode")),
            };
            let p = child_path(&path, "time-offset");
            let offset_seconds = Field {
                value: get(block, "time-offset", &path)?,
                path: &p,
            }
            .float64()?;
            let p = child_path(&path, "params");
            let params = parameters(get(block, "params", &path)?, &p)?;
            let p = child_path(&path, "emotions");
            let em = emotions(get(block, "emotions", &path)?, &p)?;
            let p = child_path(&path, "sentence-list");
            let values = Field {
                value: get(block, "sentence-list", &path)?,
                path: &p,
            }
            .array(10000)?;
            let mut sentences = Vec::with_capacity(values.len());
            let mut text_bytes = 0usize;
            for (index, value) in values.iter().enumerate() {
                let sentence =
                    manual::decode(value, index as u64 + 1, &child_path(&p, &index.to_string()))?;
                text_bytes = text_bytes
                    .checked_add(sentence.state().text.len())
                    .filter(|n| *n <= crate::app::sentence_edit::MAX_TEXT_BYTES)
                    .ok_or_else(|| format!("{p}: block text limit"))?;
                sentences.push(sentence);
            }
            let p = child_path(&path, "sentence-ranges");
            let ranges = Field {
                value: get(block, "sentence-ranges", &path)?,
                path: &p,
            }
            .array(10000)?;
            let sentence_ranges = ranges
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    Field {
                        value,
                        path: &child_path(&p, &index.to_string()),
                    }
                    .pair()
                })
                .collect::<Result<Vec<_>, _>>()?;
            // Preserve raw pairs; no repair of stale native ranges is implicit.
            blocks.push(NativeBlockDraft {
                voice,
                offset_mode,
                offset_seconds,
                parameters: params,
                emotions: em,
                sentences,
                sentence_ranges,
            });
        }
        Ok(Self {
            global,
            blocks,
            root: root.clone(),
            native: native.clone(),
        })
    }

    /// A narrow native-tree edit, not an own-project serialization shortcut.
    pub fn overlay_manual(
        &self,
        block: usize,
        sentence: usize,
        words: &[WordState],
    ) -> Result<NativeProject, String> {
        let source = self
            .blocks
            .get(block)
            .and_then(|b| b.sentences.get(sentence))
            .ok_or("Native sentence owner not found")?;
        let replacement = source.overlay_manual(words)?;
        if replacement == self.root["project"]["blocks"][block]["sentence-list"][sentence] {
            return Ok(self.native.clone());
        }
        let mut root = self.root.clone();
        root["project"]["blocks"][block]["sentence-list"][sentence] = replacement;
        let mut bytes = serde_json::to_vec(&root).map_err(|e| e.to_string())?;
        bytes.push(0);
        NativeProject::parse(&bytes)
    }
}
impl NativeBlockDraft {
    pub fn sentences(&self) -> &[NativeSentenceDraft] {
        &self.sentences
    }
}

pub(crate) fn overlay_sentence(
    original: &serde_json::Value,
    sentence: &crate::app::document::SentenceState,
) -> Result<serde_json::Value, String> {
    let draft = manual::decode(original, sentence.id, "/native-sentence")?;
    if draft.state().text != sentence.text || sentence.ssml.is_some() {
        return Err(
            "Native Save cannot yet preserve changed sentence text or pending SSML annotations"
                .into(),
        );
    }
    draft.overlay_manual(&sentence.words)
}
