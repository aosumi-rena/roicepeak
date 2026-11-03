#[path = "word_alignment.rs"]
pub mod alignment;
#[path = "block_merge.rs"]
mod block_merge;
#[path = "block_state.rs"]
mod block_state;
#[path = "control_value.rs"]
mod control_value;
#[path = "ssml_annotations.rs"]
pub(crate) mod ssml;
#[path = "document_state.rs"]
pub mod state;
pub use control_value::ControlValue;
use serde::{Deserialize, Serialize};
pub use state::{SentenceState, WordState};
#[path = "voice_ref.rs"]
mod voice_ref;
use std::{collections::BTreeMap, fs, io::Read, path::Path};
pub use voice_ref::{VoiceRef, VoiceVersion};
pub(crate) use voice_ref::{VoiceSourceInfo, resolve_source};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Parameters {
    pub speed: ControlValue,
    pub pitch: ControlValue,
    pub pause: ControlValue,
    pub volume: ControlValue,
    pub emotions: BTreeMap<String, ControlValue>,
}
impl Default for Parameters {
    fn default() -> Self {
        Self {
            speed: ControlValue::ONE,
            pitch: ControlValue::ZERO,
            pause: ControlValue::ONE,
            volume: ControlValue::ONE,
            emotions: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    pub sentence: crate::app::sentences::SentenceRef,
    pub symbol: String,
    pub reading: String,
    pub duration: f32,
    pub intonation: f32,
    pub high: bool,
    pub accent_code: u32,
    /// Explicit user intent, separate from the current derived phone display.
    pub accent_edited: bool,
    pub intonation_delta: f32,
    pub raw_query_intonation: f32,
    pub token: usize,
    pub mora: usize,
    pub phone: usize,
    pub nucleus: bool,
    pub token_text: String,
    pub duration_edit: state::DurationState,
    pub raw_query_duration_seconds: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Block {
    pub id: u64,
    #[serde(deserialize_with = "crate::app::native_origin::required_owner")]
    pub(crate) native_owner: Option<crate::app::native_origin::NativeOwner>,
    pub text: String,
    pub narrator: VoiceRef,
    pub offset_mode: OffsetMode,
    pub offset_seconds: f64,
    /// Derived display position; placement uses offset_mode and offset_seconds.
    pub timestamp: f64,
    pub parameters: Parameters,
    pub segments: Vec<Segment>,
    pub sentences: Vec<SentenceState>,
    pub sentence_serial: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OffsetMode {
    Absolute,
    PreviousStart,
    #[default]
    PreviousEnd,
}

impl Block {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            native_owner: None,
            text: String::new(),
            narrator: VoiceRef::default(),
            offset_mode: OffsetMode::PreviousEnd,
            offset_seconds: 0.0,
            timestamp: 0.0,
            parameters: Parameters::default(),
            segments: Vec::new(),
            sentences: Vec::new(),
            sentence_serial: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub format: u32,
    #[serde(deserialize_with = "crate::app::native_origin::required_document")]
    pub(crate) native_origin: Option<crate::app::native_origin::NativeOrigin>,
    pub global: Parameters,
    pub narrator_settings: BTreeMap<String, Parameters>,
    pub blocks: Vec<Block>,
    pub export_settings: crate::app::export::Settings,
}
impl Default for Project {
    fn default() -> Self {
        Self {
            format: 12,
            native_origin: None,
            global: Parameters::default(),
            narrator_settings: BTreeMap::new(),
            blocks: vec![Block::new(1)],
            export_settings: crate::app::export::Settings::default(),
        }
    }
}
impl Project {
    pub(crate) fn same_content(&self, other: &Self) -> bool {
        self.format == other.format
            && self.native_origin == other.native_origin
            && self.global == other.global
            && self.narrator_settings == other.narrator_settings
            && self.blocks == other.blocks
    }
    pub fn next_id(&self) -> u64 {
        let used: std::collections::BTreeSet<_> = self.blocks.iter().map(|b| b.id).collect();
        (1..=used.len() as u64 + 1)
            .find(|id| !used.contains(id))
            .unwrap()
    }
    pub fn read(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Project exceeds 16 MiB".into());
        }
        let project: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if project.format != 12 || project.blocks.len() > 10000 {
            return Err("Unsupported project format or block count".into());
        }
        project.validate()?;
        Ok(project)
    }
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let mut snapshot = self.clone();
        snapshot.synchronize_sentences()?;
        snapshot.validate()?;
        let bytes = serde_json::to_vec_pretty(&snapshot).map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Project exceeds 16 MiB".into());
        }
        // The file chooser handles overwrite confirmation.
        fs::write(path, bytes).map_err(|e| e.to_string())
    }
    pub fn validate(&self) -> Result<(), String> {
        self.export_settings.validate_stored()?;
        match &self.native_origin {
            Some(origin) => origin.validate_links(
                self.blocks.iter().map(|b| b.native_owner),
                self.blocks
                    .iter()
                    .flat_map(|b| b.sentences.iter().map(|s| s.native_owner)),
            )?,
            None if self.blocks.iter().any(|b| {
                b.native_owner.is_some() || b.sentences.iter().any(|s| s.native_owner.is_some())
            }) =>
            {
                return Err("Native owner without native origin".into());
            }
            None => (),
        }
        if self.format != 12 || self.blocks.len() > 10000 {
            return Err("Unsupported project format or block count".into());
        }
        let validate_parameters = |p: &Parameters| {
            if p.volume.normalized() < 0.0 {
                return Err("Volume cannot be negative".to_owned());
            }
            if [p.speed, p.pitch, p.pause, p.volume]
                .iter()
                .any(|v| !v.displayable())
                || p.emotions.values().any(|v| !v.displayable())
            {
                return Err("Non-finite project parameter".to_owned());
            }
            if p.emotions.len() > 1024 || p.emotions.keys().any(|k| k.len() > 4096) {
                return Err("Project emotion count or label limit".to_owned());
            }
            Ok(())
        };
        validate_parameters(&self.global)?;
        if self.narrator_settings.len() > 256 {
            return Err("Narrator settings exceed the voice count limit".into());
        }
        for (key, parameters) in &self.narrator_settings {
            if key.is_empty() || key.len() > 4096 {
                return Err("Invalid narrator settings identity".into());
            }
            validate_parameters(parameters)?;
        }
        let mut ids = std::collections::BTreeSet::new();
        for block in &self.blocks {
            if !ids.insert(block.id)
                || !block.offset_seconds.is_finite()
                || !block.timestamp.is_finite()
                || block.timestamp < 0.0
                || block.text.len() > 1_000_000
                || block.text.contains('\0')
                || !block.narrator.valid()
                || block.segments.len() > 100000
            {
                return Err("Invalid project block".into());
            }
            validate_parameters(&block.parameters)?;
            block.validate_sentences()?;
            let sentences = block.sentence_ranges();
            if block.segments.iter().any(|s| {
                sentences.get(s.sentence.index) != Some(&s.sentence)
                    || !s.duration.is_finite()
                    || s.duration < 0.0
                    || !s.intonation.is_finite()
                    || !s.raw_query_intonation.is_finite()
                    || !s.intonation_delta.is_finite()
                    || !s.duration_edit.valid()
                    || !s.raw_query_duration_seconds.is_finite()
                    || s.raw_query_duration_seconds < 0.0
                    || s.token_text.len() > 1_000_000
                    || s.symbol.len() > 4096
                    || s.reading.len() > 4096
            }) {
                return Err("Invalid phoneme segment".into());
            }
        }
        Ok(())
    }
    pub fn validate_controls(
        &self,
        policy: roicepeak::scalar_layers::ValuePolicy,
    ) -> Result<(), String> {
        let parameters = |p: &Parameters, correction: bool| {
            if !policy.permits(p.speed.normalized(), 0.5, 2.0)
                || !policy.permits(p.pitch.normalized(), -1.5, 1.5)
                || !policy.permits(p.pause.normalized(), 0.5, 2.0)
                || !policy.permits_nonnegative(p.volume.normalized(), 0.0, 2.0)
                || p.emotions.values().any(|v| {
                    !policy.permits(v.normalized(), if correction { -1.0 } else { 0.0 }, 1.0)
                })
            {
                return Err("Project controls exceed the enabled value policy".to_owned());
            }
            if p.speed.normalized() == 0.0 {
                return Err("Speed cannot be zero for synthesis".to_owned());
            }
            Ok(())
        };
        parameters(&self.global, true)?;
        for p in self.narrator_settings.values() {
            parameters(p, true)?;
        }
        for block in &self.blocks {
            parameters(&block.parameters, false)?;
        }
        Ok(())
    }
    pub fn import_text(&mut self, text: &str) {
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let mut block = Block::new(self.next_id());
            block.text = line.trim().to_owned();
            self.blocks.push(block);
        }
    }
}

pub fn timestamp(seconds: f64) -> String {
    let centiseconds = (seconds.max(0.0) * 100.0).round() as u64;
    format!(
        "{:02}.{:02}.{:02}",
        centiseconds / 6000,
        centiseconds / 100 % 60,
        centiseconds % 100
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_round_trip_rejects_global_corruption_and_duplicate_ids() {
        let mut project = Project::default();
        project.import_text("第一行\nsecond line");
        let bytes = serde_json::to_vec(&project).unwrap();
        let decoded: Project = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(project, decoded);
        assert!(decoded.validate().is_ok());
        project.global.speed = ControlValue::from_normalized(f32::NAN);
        assert!(project.validate().is_err());
        project.global.speed = ControlValue::ONE;
        project.blocks[1].id = project.blocks[0].id;
        assert!(project.validate().is_err());
        project.blocks[0].id = u64::MAX;
        assert_ne!(project.next_id(), u64::MAX);
    }
}
