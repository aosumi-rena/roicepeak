//! Native-origin sentence-list projection for complete-sentence deletions.
use crate::app::{
    document::Block,
    native_adapter,
    native_origin::{NativeOrigin, NativeOwner},
    sentences,
};
use serde_json::Value;
use std::collections::BTreeMap;

fn source_text(values: &[Value]) -> Result<String, String> {
    values
        .iter()
        .map(|value| {
            value["text"]
                .as_str()
                .ok_or_else(|| "Native source sentence text missing".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.concat())
}

fn matches_partition<'a>(text: &str, texts: impl IntoIterator<Item = &'a str>) -> bool {
    let partition = sentences::partition(text);
    let texts = texts.into_iter().collect::<Vec<_>>();
    partition.len() == texts.len()
        && partition
            .iter()
            .zip(texts)
            .all(|(range, expected)| text.get(range.bytes.clone()) == Some(expected))
}

pub(super) fn overlay(
    origin: &NativeOrigin,
    block_owner: NativeOwner,
    block: &Block,
    raw: &mut Value,
) -> Result<(), String> {
    let originals = raw["sentence-list"]
        .as_array()
        .ok_or("Native source sentence list missing")?;
    let original_text = source_text(originals)?;
    let mut remainder = block.text.as_str();
    for sentence in &block.sentences {
        remainder = remainder
            .strip_prefix(sentence.text.as_str())
            .ok_or("Native Save requires synchronized sentence text")?;
    }
    if !remainder.is_empty() {
        return Err("Native Save requires synchronized sentence text".into());
    }
    let mut unchanged_structure = originals.len() == block.sentences.len();
    if unchanged_structure {
        for (index, sentence) in block.sentences.iter().enumerate() {
            if Some(origin.sentence_owner_in_block(block_owner, index)?) != sentence.native_owner {
                unchanged_structure = false;
                break;
            }
        }
    }
    if !unchanged_structure {
        // This branch accepts a strict subsequence only: no fresh, moved, split,
        // reordered or text-substituted sentence has a qualified writer here.
        if block.sentences.is_empty() {
            return Err(
                "Native Save cannot yet qualify deleting the last sentence of a block".into(),
            );
        }
        if block.sentences.len() >= originals.len() {
            return Err(
                "Native Save supports only complete-sentence deletion for changed structure".into(),
            );
        }
        if !matches_partition(
            &original_text,
            originals
                .iter()
                .map(|value| value["text"].as_str().unwrap()),
        ) || !matches_partition(
            &block.text,
            block
                .sentences
                .iter()
                .map(|sentence| sentence.text.as_str()),
        ) {
            return Err(
                "Native Save sentence deletion requires ordinary delimiter-partition boundaries"
                    .into(),
            );
        }
        let indices = (0..originals.len())
            .map(|index| {
                origin
                    .sentence_owner_in_block(block_owner, index)
                    .map(|owner| (owner, index))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let mut previous = None;
        for sentence in &block.sentences {
            let index = sentence
                .native_owner
                .and_then(|owner| indices.get(&owner).copied())
                .ok_or("Native Save cannot yet add or move a sentence into this block")?;
            if previous.is_some_and(|last| index <= last) {
                return Err("Native Save cannot yet reorder or duplicate sentence owners".into());
            }
            if originals[index]["text"].as_str() != Some(sentence.text.as_str()) {
                return Err(
                    "Native Save cannot yet regenerate changed sentence text or word ranges".into(),
                );
            }
            previous = Some(index);
        }
    } else if original_text != block.text {
        return Err("Native Save cannot yet regenerate changed text or word ranges".into());
    }
    let values = block
        .sentences
        .iter()
        .map(|sentence| {
            let owner = sentence
                .native_owner
                .ok_or("Native Save cannot yet create a sentence")?;
            native_adapter::overlay_sentence(origin.sentence(owner)?, sentence)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let ranges = if unchanged_structure {
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
    // All fallible work finishes before modifying the caller's cloned raw block.
    raw["sentence-list"] = Value::Array(values);
    if let Some(ranges) = ranges {
        raw["sentence-ranges"] = Value::Array(ranges);
    }
    Ok(())
}
