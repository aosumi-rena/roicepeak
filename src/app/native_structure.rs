//! Owner-bound native blocks and sentence composition.
use super::fresh;
use crate::app::{
    document::{Block, Project, SentenceState},
    native_adapter,
    native_origin::{NativeOrigin, NativeOwner},
    sentences,
};
use serde_json::Value;

fn matches_partition(block: &Block) -> bool {
    let partition = sentences::partition(&block.text);
    partition.len() == block.sentences.len()
        && partition
            .iter()
            .zip(&block.sentences)
            .all(|(range, sentence)| {
                block.text.get(range.bytes.clone()) == Some(sentence.text.as_str())
            })
}

pub(super) fn requires_preparation(project: &Project) -> Result<bool, String> {
    project.validate()?;
    if project.blocks.iter().any(|block| {
        block.text.trim().is_empty()
            && block
                .sentences
                .iter()
                .any(|sentence| sentence.ssml.is_some())
    }) {
        return Err(
            "Pending pronunciation on whitespace-only text cannot be prepared for Native Save"
                .into(),
        );
    }
    for block in &project.blocks {
        if block.text.trim().is_empty() {
            continue;
        }
        if block.native_owner.is_none() || block.sentences.is_empty() {
            return Ok(true);
        }
        for sentence in &block.sentences {
            if sentence.ssml.is_some() {
                return Ok(true);
            }
            if sentence.text.trim().is_empty() {
                continue;
            }
            let (Some(origin), Some(owner)) =
                (project.native_origin.as_ref(), sentence.native_owner)
            else {
                return Ok(true);
            };
            let original = origin.sentence(owner)?;
            if original["text"].as_str() != Some(sentence.text.as_str())
                || !native_adapter::preserves_pronunciation_input(original, sentence)?
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
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
            // Validation uses the whole accepted query; the retained overlay
            // still owns its original opaque word/mora/phone properties.
            fresh::replacement_sentence(current)?;
        }
        return native_adapter::overlay_sentence(original, current);
    }
    // The old token nodes are removed as a whole. No token subtree is guessed
    // to belong to a different lexical or phonological sequence.
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
    if !unchanged && !matches_partition(block) {
        return Err(
            "Native Save requires ordinary sentence boundaries for changed structure or text"
                .into(),
        );
    }
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
            sentences::partition(&block.text)
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
