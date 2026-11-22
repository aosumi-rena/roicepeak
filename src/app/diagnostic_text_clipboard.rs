use crate::app::{
    catalog::Catalog, project_files, render::Config, resources::Locations, sentence_edit,
    text_clipboard::Payload,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    path::PathBuf,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    block: u64,
    selection: [usize; 2],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    locations: PathBuf,
    project: PathBuf,
    block: u64,
    selection: [usize; 2],
    #[serde(default)]
    paste: Option<Target>,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-text-clipboard accepts a JSON object with locations and project .vpp paths on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Clipboard diagnostic input exceeds 16 MiB".into());
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let locations = Locations::read(&input.locations)?;
    let catalog = Catalog::load_roots(&locations.voice_roots)?;
    let mut config = Config::from_catalog(locations.dictionary.clone(), &catalog)?;
    config.value_policy = locations.value_policy();
    let mut project = project_files::read(&input.project, Some(&config))?;
    let source = project
        .blocks
        .iter()
        .find(|block| block.id == input.block)
        .ok_or("Clipboard source block is absent")?;
    let selection = input.selection[0]..input.selection[1];
    let payload = Payload::from_block(source, selection)?;
    let mut updated = None;
    let mut caret = None;
    let mut wire_bytes = 0;
    if let Some(payload) = &payload {
        let wire = payload.encode()?;
        wire_bytes = wire.len();
        let decoded = Payload::decode(&wire)?;
        if &decoded != payload {
            return Err("Clipboard payload roundtrip differs".into());
        }
        if let Some(target) = input.paste {
            let index = project
                .blocks
                .iter()
                .position(|block| block.id == target.block)
                .ok_or("Clipboard target block is absent")?;
            let replacement = sentence_edit::replace_rich(
                &project.blocks[index],
                target.selection[0]..target.selection[1],
                &decoded.sentences,
            )?;
            caret = Some(
                target.selection[0]
                    .checked_add(decoded.text().chars().count())
                    .ok_or("Clipboard caret overflow")?,
            );
            project.blocks[index] = replacement;
            project.validate()?;
            updated = Some(project.blocks[index].clone());
        }
    }
    let value = json!({"copied_text": payload.as_ref().map(Payload::text).unwrap_or_default(),
        "payload": payload, "wire_bytes": wire_bytes, "updated_block": updated, "caret": caret});
    let mut bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| error.to_string())
}
