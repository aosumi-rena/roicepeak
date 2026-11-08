//! Owner-bound native blocks and sentence composition.
use super::fresh;
use crate::app::{
    document::{Block, Project, SentenceState},
    native_adapter,
    native_origin::{NativeOrigin, NativeOwner},
};
use serde_json::Value;

pub(super) fn requires_preparation(project: &Project) -> Result<bool, String> {
    crate::app::save_lexical::required(project)
}

fn sentence(origin: &NativeOrigin, current: &SentenceState) -> Result<Value, String> {
    let Some(owner) = current.native_owner else {
        return fresh::new_sentence(current);
    };
    let original = origin.sentence(owner)?;
    let same_words = native_adapter::preserves_word_structure(original, current)?;
    let changed_text = original["text"].as_str() != Some(current.text.as_str());
    if current.text.trim().is_empty() && (changed_text || !same_words) {
        let replacement = fresh::replacement_sentence(current)?;
        let mut value = original.clone();
        for key in ["text", "has-eos", "tokens"] {
            value[key] = replacement[key].clone();
        }
        return Ok(value);
    }
    if same_words {
        if !current.text.trim().is_empty()
            && (changed_text || !native_adapter::preserves_pronunciation_input(original, current)?)
        {
            // Validate changed words before preserving their opaque properties.
            fresh::replacement_sentence(current)?;
        }
        return native_adapter::overlay_sentence(original, current);
    }
    // Replace obsolete tokens as a whole.
    let replacement = fresh::replacement_sentence(current)?;
    let mut value = original.clone();
    for key in ["text", "has-eos", "tokens"] {
        value[key] = replacement[key].clone();
    }
    Ok(value)
}

pub(super) fn overlay(
    origin: &NativeOrigin,
    block_owner: Option<NativeOwner>,
    block: &Block,
    raw: &mut Value,
) -> Result<(), String> {
    let originals = raw["sentence-list"]
        .as_array()
        .ok_or("Native source sentence list missing")?;
    let mut remainder = block.text.as_str();
    for sentence in &block.sentences {
        remainder = remainder
            .strip_prefix(sentence.text.as_str())
            .ok_or("Native Save requires synchronized sentence text")?;
    }
    if !remainder.is_empty() {
        return Err("Native Save requires synchronized sentence text".into());
    }
    let unchanged = if let Some(owner) = block_owner {
        if originals.len() != block.sentences.len() {
            false
        } else {
            let mut same = true;
            for (index, current) in block.sentences.iter().enumerate() {
                if current.native_owner != Some(origin.sentence_owner_in_block(owner, index)?)
                    || originals[index]["text"].as_str() != Some(current.text.as_str())
                {
                    same = false;
                    break;
                }
            }
            same
        }
    } else {
        false
    };
    if block.text.trim().is_empty()
        && block
            .segments
            .iter()
            .any(|s| s.accent_edited || s.intonation_delta != 0.0 || s.duration_edit.edited())
    {
        for current in &block.sentences {
            let replaces = if let Some(owner) = current.native_owner {
                let original = origin.sentence(owner)?;
                original["text"].as_str() != Some(current.text.as_str())
                    || !native_adapter::preserves_word_structure(original, current)?
            } else {
                true
            };
            if replaces {
                return Err(
                    "Whitespace-only text cannot retain authored pronunciation controls".into(),
                );
            }
        }
    }
    let values = block
        .sentences
        .iter()
        .map(|s| sentence(origin, s))
        .collect::<Result<Vec<_>, _>>()?;
    let ranges = if unchanged {
        None
    } else {
        Some(
            block
                .sentence_ranges()
                .iter()
                .map(|range| {
                    let first = i32::try_from(range.chars.start)
                        .map_err(|_| "Native sentence start exceeds int32")?;
                    let last = i32::try_from(range.chars.end)
                        .map_err(|_| "Native sentence end exceeds int32")?;
                    Ok(serde_json::json!([first, last]))
                })
                .collect::<Result<Vec<_>, String>>()?,
        )
    };
    raw["sentence-list"] = Value::Array(values);
    if let Some(ranges) = ranges {
        raw["sentence-ranges"] = Value::Array(ranges);
    }
    Ok(())
}
