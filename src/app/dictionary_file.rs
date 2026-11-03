use super::{Entry, record};
use serde_json::Value;
use std::{collections::BTreeSet, fs, io::Read, path::Path};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;

pub(super) fn bounded(value: &Value) -> Result<(), String> {
    let mut remaining = 200_000usize;
    fn visit(value: &Value, depth: usize, remaining: &mut usize) -> Result<(), String> {
        if depth > 64 || *remaining == 0 {
            return Err("Dictionary nesting or value count limit".into());
        }
        *remaining -= 1;
        match value {
            Value::Array(values) => {
                for value in values {
                    visit(value, depth + 1, remaining)?;
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    visit(value, depth + 1, remaining)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    visit(value, 0, &mut remaining)
}

fn decode_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let big_endian = bytes[0] == 0xfe;
        let payload = &bytes[2..];
        // Only complete UTF-16 code units are decoded.
        let complete = payload.len() & !1usize;
        let units = payload[..complete].chunks_exact(2).map(|p| {
            if big_endian {
                u16::from_be_bytes([p[0], p[1]])
            } else {
                u16::from_le_bytes([p[0], p[1]])
            }
        });
        return char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .map_err(|_| "Dictionary contains invalid UTF-16".into());
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.into());
    }
    Ok(encoding_rs::WINDOWS_1252
        .decode_without_bom_handling(bytes)
        .0
        .into_owned())
}

pub(super) fn parse(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Dictionary exceeds 8 MiB".into());
    }
    let text = decode_text(bytes)?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid dictionary JSON: {e}"))?;
    bounded(&value)?;
    let Value::Array(values) = value else {
        return Err("Dictionary must contain a JSON array".into());
    };
    if values.len() > MAX_ENTRIES {
        return Err("Dictionary entry count exceeds 10000".into());
    }
    let entries = values
        .into_iter()
        .map(record::decode)
        .collect::<Result<Vec<_>, _>>()?;
    for entry in &entries {
        entry.validate_stored()?;
    }
    Ok(entries)
}

pub(super) fn read(path: &Path) -> Result<Vec<Entry>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    parse(&bytes)
}

pub(super) fn encode(entries: &[Entry]) -> Result<Vec<u8>, String> {
    if entries.len() > MAX_ENTRIES {
        return Err("Dictionary entry count exceeds 10000".into());
    }
    let values = entries
        .iter()
        .map(Entry::native_value)
        .collect::<Result<Vec<_>, _>>()?;
    let value = Value::Array(values);
    bounded(&value)?;
    let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Dictionary exceeds 8 MiB".into());
    }
    Ok(bytes)
}

pub(super) fn write(path: &Path, entries: &[Entry]) -> Result<(), String> {
    fs::write(path, encode(entries)?).map_err(|e| e.to_string())
}

fn identity(entry: &Entry) -> (i32, usize, &str) {
    // Opaque rows retain their partially decoded fields, including language zero.
    (0, entry.part_of_speech, entry.word.as_str())
}

pub(super) fn merge(old: &[Entry], incoming: &[Entry]) -> Result<Vec<Entry>, String> {
    let imported = incoming.iter().map(identity).collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for entry in old
        .iter()
        .filter(|entry| !imported.contains(&identity(entry)))
        .chain(incoming)
    {
        if seen.insert(identity(entry)) {
            result.push(entry.clone());
        }
        if result.len() > MAX_ENTRIES {
            return Err("Merged dictionary entry count exceeds 10000".into());
        }
    }
    Ok(result)
}
