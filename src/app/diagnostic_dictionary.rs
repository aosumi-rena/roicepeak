use base64::Engine;
use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    bytes_base64: String,
    merge_with_base64: Option<String>,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-user-dictionary accepts JSON on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(24 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 24 * 1024 * 1024 {
        return Err("Dictionary inspection stdin exceeds 24 MiB".into());
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let decode = |text: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(|e| e.to_string())
    };
    let document = decode(&input.bytes_base64)?;
    let previous = input.merge_with_base64.as_deref().map(decode).transpose()?;
    let output = crate::app::dictionary::inspect_native(&document, previous.as_deref())?;
    let output = serde_json::to_vec(&json!({"result": output})).map_err(|e| e.to_string())?;
    if output.len() > 32 * 1024 * 1024 {
        return Err("Dictionary inspection output exceeds 32 MiB".into());
    }
    io::stdout()
        .lock()
        .write_all(&output)
        .map_err(|e| e.to_string())
}
