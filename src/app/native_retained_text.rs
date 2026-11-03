//! Current analyzed ranges for retained native sentence and word identities.
use crate::app::document::SentenceState;
use serde_json::Value;

fn boundary(pos: u32) -> bool {
    matches!(pos, 0x2000 | 0x2001)
}

pub(super) fn overlay(value: &mut Value, sentence: &SentenceState) -> Result<(), String> {
    if !sentence.query.matches(&sentence.text, &sentence.words)
        || sentence.word_positions.source != sentence.text
        || sentence.query.raw.len() != sentence.words.len()
    {
        return Err("Prepare the current text before saving this edited native sentence".into());
    }
    sentence.word_positions.validate(&sentence.words)?;
    let boundaries = sentence
        .text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(sentence.text.len()))
        .collect::<Vec<_>>();
    let mut pairs = Vec::new();
    let mut previous = (0, 0);
    for ((raw, word), range) in sentence
        .query
        .raw
        .iter()
        .zip(&sentence.words)
        .zip(&sentence.word_positions.ranges)
    {
        if raw.surface != word.surface || raw.language != word.language || raw.pos != word.pos {
            return Err("Native word positions no longer match the analyzed words".into());
        }
        if boundary(word.pos) {
            if range.is_some() {
                return Err("Native synthetic boundary cannot own text".into());
            }
            continue;
        }
        let range = range
            .as_ref()
            .ok_or("Native Save requires a current position for every retained word")?;
        if range.start < previous.0 || range.end < previous.1 {
            return Err("Native word positions are not in source order".into());
        }
        previous = (range.start, range.end);
        let pair = |first: usize, last: usize| -> Result<Value, String> {
            Ok(serde_json::json!([
                i32::try_from(first).map_err(|_| "Native word position exceeds int32")?,
                i32::try_from(last).map_err(|_| "Native word position exceeds int32")?,
            ]))
        };
        let start = *boundaries
            .get(range.start)
            .ok_or("Native word start is outside current text")?;
        let end = *boundaries
            .get(range.end)
            .ok_or("Native word end is outside current text")?;
        pairs.push((pair(start, end)?, pair(range.start, range.end)?));
    }
    let tokens = value["tokens"]
        .as_array_mut()
        .ok_or("Native retained sentence token list missing")?;
    if tokens.len() != pairs.len() {
        return Err("Native Save cannot yet create or remove words".into());
    }
    // Validate the complete projection before modifying this staged sentence.
    for (token, (bytes, scalars)) in tokens.iter_mut().zip(pairs) {
        token["r8"] = bytes;
        token["r32"] = scalars;
    }
    value["text"] = sentence.text.clone().into();
    Ok(())
}
