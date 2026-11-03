use crate::app::{document::Project, sentence_edit, text_clipboard::Payload};
use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
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
    project: Project,
    block: u64,
    selection: [usize; 2],
    #[serde(default)]
    paste: Option<Target>,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-text-clipboard accepts one JSON object on stdin".into());
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
    let mut input: Input = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    input.project.validate()?;
    let source = input
        .project
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
            let index = input
                .project
                .blocks
                .iter()
                .position(|block| block.id == target.block)
                .ok_or("Clipboard target block is absent")?;
            let replacement = sentence_edit::replace(
                &input.project.blocks[index],
                target.selection[0]..target.selection[1],
                &decoded.sentences,
            )?;
            caret = Some(
                target.selection[0]
                    .checked_add(decoded.text().chars().count())
                    .ok_or("Clipboard caret overflow")?,
            );
            input.project.blocks[index] = replacement;
            input.project.validate()?;
            updated = Some(input.project);
        }
    }
    let value = json!({"copied_text": payload.as_ref().map(Payload::text).unwrap_or_default(),
        "payload": payload, "wire_bytes": wire_bytes, "updated_project": updated, "caret": caret});
    let mut bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| error.to_string())
}
