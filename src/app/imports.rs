//! Bounded script and subtitle imports.
mod csv;
mod srt;
mod srt_project;
mod ssml;
mod ssml_project;
mod text;
pub use srt::Captions;

use crate::app::document::{Block, Parameters, Project, VoiceRef};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
};

pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_BLOCKS: usize = 10_000;
pub const MAX_TEXT: usize = 1_000_000;

pub struct ImportedBlock {
    pub text: String,
    pub narrator: Option<String>,
    pub start: Option<f64>,
    pub parameters: Option<Parameters>,
    pub source_line: usize,
}

pub struct Batch {
    pub blocks: Vec<ImportedBlock>,
    pub warnings: Vec<String>,
}

/// Use caller-supplied narrator names and presets without implicit substitution.
#[derive(Default)]
pub struct CsvCatalog {
    pub narrators: BTreeSet<String>,
    pub default_narrator: Option<String>,
    pub presets: BTreeMap<(String, String), Parameters>,
}

pub fn read(path: &Path) -> Result<String, String> {
    let bytes = read_bytes(path)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| "Import must be UTF-8; convert legacy encodings before import".to_owned())?;
    check(&text)?;
    Ok(text)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES as u64 {
        return Err("Import must be a regular file of at most 16 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Import grew beyond 16 MiB".into());
    }
    Ok(bytes)
}

pub fn parse_srt(text: &str) -> Result<Captions, String> {
    srt::parse(text)
}
pub fn read_srt(path: &Path) -> Result<Captions, String> {
    parse_srt(&text::decode(&read_bytes(path)?)?)
}
pub fn parse_csv(text: &str, catalog: &CsvCatalog) -> Result<Batch, String> {
    csv::parse(text, catalog)
}

/// Append in source order, validating timed records before changing the project.
pub fn append(
    project: &Project,
    batch: Batch,
    default_narrator: Option<&VoiceRef>,
) -> Result<(Project, Vec<String>), String> {
    project.validate()?;
    if project
        .blocks
        .len()
        .checked_add(batch.blocks.len())
        .is_none_or(|n| n > MAX_BLOCKS)
    {
        return Err("Import exceeds 10000 project blocks".into());
    }
    let mut proposed = project.clone();
    let mut warnings = batch.warnings;
    let mut missing_narrators = 0usize;
    let mut used = proposed
        .blocks
        .iter()
        .map(|b| b.id)
        .collect::<BTreeSet<_>>();
    let mut next_id = 1u64;
    for item in batch.blocks {
        if item.text.is_empty() || item.text.len() > MAX_TEXT {
            return Err(line_error(item.source_line, "invalid imported text length"));
        }
        while used.contains(&next_id) {
            next_id = next_id.checked_add(1).ok_or("Block identifier overflow")?;
        }
        let mut block = Block::new(next_id);
        used.insert(next_id);
        block.text = item.text;
        block.narrator = item
            .narrator
            .map(|identity| {
                VoiceRef::latest(
                    identity,
                    default_narrator.map_or("japanese", |reference| reference.language.as_str()),
                )
            })
            .or_else(|| default_narrator.cloned())
            .unwrap_or_default();
        if block.narrator.is_empty() {
            missing_narrators += 1;
        }
        if let Some(start) = item.start {
            block.offset_mode = crate::app::document::OffsetMode::Absolute;
            block.offset_seconds = start;
        }
        if let Some(parameters) = item.parameters {
            block.parameters = parameters;
        }
        proposed.blocks.push(block);
    }
    if missing_narrators != 0 {
        warnings.push(format!(
            "{missing_narrators} imported blocks need an explicit narrator selection."
        ));
    }
    proposed.validate()?;
    let bytes = serde_json::to_vec(&proposed).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Imported project would exceed 16 MiB".into());
    }
    Ok((proposed, warnings))
}

pub(super) fn check(text: &str) -> Result<(), String> {
    if text.len() > MAX_BYTES {
        return Err("Import exceeds 16 MiB".into());
    }
    if text.contains('\0') {
        return Err("NUL is not allowed in imported text".into());
    }
    Ok(())
}

pub(super) fn lines(text: &str) -> Result<Vec<&str>, String> {
    check(text)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // Bound line bookkeeping as well as file bytes.
    if text.bytes().filter(|&b| b == b'\n').take(100_001).count() > 100_000 {
        return Err("Import exceeds 100000 lines".into());
    }
    let lines = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect::<Vec<_>>();
    if lines.iter().any(|line| line.contains('\r')) {
        return Err("Use LF or CRLF line endings for import".into());
    }
    Ok(lines)
}

pub(super) fn line_error(line: usize, message: &str) -> String {
    format!("Import line {line}: {message}")
}

#[derive(Clone, Copy)]
pub enum Format {
    Csv,
}

impl crate::app::editor::EditorApp {
    pub(crate) fn import_format(&mut self, format: Format) -> Result<(), String> {
        let chooser = match format {
            Format::Csv => {
                rfd::FileDialog::new().add_filter("UTF-8 narrator script", &["csv", "txt"])
            }
        };
        let Some(path) = chooser.pick_file() else {
            return Ok(());
        };
        let text = read(&path)?;
        let default_key = self.append_narrator(None);
        let default_narrator = self
            .catalog
            .voice(&default_key)
            .map(|voice| voice.name.clone());
        let batch = match format {
            Format::Csv => {
                let names = self
                    .catalog
                    .voices
                    .iter()
                    .map(|voice| voice.name.clone())
                    .collect::<BTreeSet<_>>();
                parse_csv(
                    &text,
                    &CsvCatalog {
                        narrators: names,
                        default_narrator: default_narrator.clone(),
                        presets: BTreeMap::new(),
                    },
                )?
            }
        };
        let first = self.project.blocks.len();
        let (project, warnings) = append(
            &self.project,
            batch,
            (!default_key.is_empty()).then_some(&default_key),
        )?;
        self.project = project;
        self.select_only(first);
        self.scroll = (first as f32 * 102.0 - 24.0).max(0.0);
        if !warnings.is_empty() {
            self.error = Some(warnings.join(" "));
        }
        Ok(())
    }
}

pub(crate) fn stage_ssml(
    source: &str,
    narrator: &VoiceRef,
    settings: crate::app::export::Settings,
) -> Result<Project, String> {
    ssml_project::replacement(ssml::parse(source)?, narrator, settings)
}
