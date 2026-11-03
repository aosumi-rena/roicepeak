//! Export names and sidecars preserve source text and prepared phone timing.
use super::{Encoding, NamePart, Scope, Settings};
use crate::app::{document::Project, render::RenderedBlock};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone)]
pub(super) struct Output {
    pub block: Option<u64>,
    pub name: String,
    pub paths: Vec<PathBuf>,
}

pub(super) fn plan(
    project: &Project,
    scope: &Scope,
    settings: &Settings,
    names: &BTreeMap<String, String>,
    directory: &std::path::Path,
) -> Result<Vec<Output>, String> {
    settings.validate(scope)?;
    let blocks: Vec<_> = match scope {
        Scope::Project if !settings.split => vec![None],
        Scope::Project => project.blocks.iter().map(Some).collect(),
        Scope::Blocks(ids) => ids
            .iter()
            .map(|id| {
                project
                    .blocks
                    .iter()
                    .find(|block| block.id == *id)
                    .map(Some)
                    .ok_or_else(|| "An export block no longer exists".to_owned())
            })
            .collect::<Result<_, _>>()?,
    };
    if blocks.is_empty() {
        return Err("Select at least one block to export".into());
    }
    let mut outputs = Vec::with_capacity(blocks.len());
    let mut unique = std::collections::BTreeSet::new();
    for (index, block) in blocks.iter().enumerate() {
        let number = if matches!(scope, Scope::Blocks(_)) {
            block
                .and_then(|block| project.blocks.iter().position(|b| b.id == block.id))
                .unwrap_or(index)
        } else {
            index
        };
        let name = filename(
            settings,
            if matches!(scope, Scope::Blocks(_)) {
                project.blocks.len()
            } else {
                blocks.len()
            },
            number,
            block
                .map(|b| {
                    names
                        .get(&b.narrator.identity)
                        .map(String::as_str)
                        .unwrap_or(&b.narrator.identity)
                })
                .unwrap_or(""),
            block.map(|b| b.text.as_str()).unwrap_or(""),
            block.is_some() && settings.split,
        );
        if name.encode_utf16().count() > 255 {
            return Err("An export filename is too long; shorten the Save as name".into());
        }
        let audio = directory.join(&name);
        let mut paths = vec![audio.clone()];
        if settings.scripts {
            for (enabled, extension) in [
                (settings.text, "txt"),
                (settings.srt, "srt"),
                (settings.lab, "lab"),
            ] {
                if enabled {
                    paths.push(audio.with_extension(extension));
                }
            }
        }
        for path in &paths {
            // Windows treats these output names without case distinctions.
            if !unique.insert(path.to_string_lossy().to_lowercase()) {
                return Err(
                    "The naming rule produces duplicate files; include the number field".into(),
                );
            }
        }
        outputs.push(Output {
            block: block.map(|b| b.id),
            name,
            paths,
        });
    }
    Ok(outputs)
}

pub(super) fn filename(
    settings: &Settings,
    total: usize,
    index: usize,
    voice: &str,
    text: &str,
    naming: bool,
) -> String {
    let mut name = String::new();
    if naming {
        let width = total.max(1).to_string().len();
        for part in settings.parts {
            let value = match part {
                NamePart::None => continue,
                NamePart::Number => format!("{index:0width$}"),
                NamePart::Voice => voice.into(),
                NamePart::Script => text.chars().take(20).collect(),
            };
            name.push_str(&value);
            name.push('-');
        }
    }
    name.push_str(&settings.name);
    name.push_str(settings.format.extension());
    name.chars()
        .filter_map(|ch| match ch {
            '\r' => None,
            '\n' | '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => Some('_'),
            '\t' | '\u{b}' | '\u{c}' => Some(' '),
            _ => Some(ch),
        })
        .collect()
}

pub(super) fn encode(text: &str, encoding: Encoding) -> Result<Vec<u8>, String> {
    match encoding {
        Encoding::Utf8 => Ok(text.as_bytes().to_vec()),
        Encoding::ShiftJis => {
            let (bytes, _, unmappable) = encoding_rs::SHIFT_JIS.encode(text);
            if unmappable {
                return Err(
                    "The script contains characters unavailable in Shift-JIS; select UTF8".into(),
                );
            }
            Ok(bytes.into_owned())
        }
    }
}

pub(super) fn text(project: &Project, selected: Option<u64>) -> String {
    project
        .blocks
        .iter()
        .filter(|block| selected.is_none_or(|id| block.id == id))
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\r\n")
}

pub(super) fn srt(
    project: &Project,
    blocks: &[RenderedBlock],
    selected: Option<u64>,
    absolute_start: Option<f64>,
) -> String {
    let mut result = String::new();
    let mut cue = 1;
    for block in blocks
        .iter()
        .filter(|b| selected.is_none_or(|id| b.id == id))
    {
        if block.segments.is_empty() {
            continue;
        }
        let Some(source) = project.blocks.iter().find(|b| b.id == block.id) else {
            continue;
        };
        let script = source
            .text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\r\n");
        let start = absolute_start.unwrap_or(block.start);
        result.push_str(&format!(
            "{cue}\r\n{} --> {}\r\n{script}\r\n\r\n",
            timestamp(start),
            timestamp(start + block.duration)
        ));
        cue += 1;
    }
    result
}

fn timestamp(seconds: f64) -> String {
    let whole = seconds.max(0.0).floor() as u64;
    let milliseconds = (seconds.max(0.0) * 1000.0).floor() as u64 % 1000;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        whole / 3600,
        whole / 60 % 60,
        whole % 60,
        milliseconds
    )
}

pub(super) fn lab(blocks: &[RenderedBlock], selected: Option<u64>, local: bool) -> String {
    let mut result = String::new();
    for block in blocks
        .iter()
        .filter(|b| selected.is_none_or(|id| b.id == id))
    {
        let origin = if local {
            block
                .timing
                .first()
                .map_or(block.start, |sentence| sentence.start)
        } else {
            0.0
        };
        for sentence in &block.timing {
            let base = sentence.start - origin;
            for phone in &sentence.phones {
                let start = base + phone.start;
                let end = start + phone.duration;
                result.push_str(&format!(
                    "{} {} {}\r\n",
                    (start * 10_000_000.0).trunc() as u64,
                    (end * 10_000_000.0).trunc() as u64,
                    phone.symbol
                ));
            }
        }
    }
    result
}
