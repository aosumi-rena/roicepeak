//! Own durability projection for unconsumed annotations, not a native file codec.
use super::{Annotation, Pending};
use crate::app::document::Block;
use std::ops::Range;

fn boundaries(text: &str) -> Vec<usize> {
    text.char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect()
}

fn neutral(text: &str, range: Range<usize>, output: &mut Vec<Annotation>) {
    let mut start = range.start;
    while start < range.end {
        while start < range.end && text.as_bytes()[start] == b' ' {
            start += 1;
        }
        let mut end = start;
        while end < range.end && text.as_bytes()[end] != b' ' {
            end += 1;
        }
        if start != end {
            output.push(Annotation {
                surface: text[start..end].to_owned(),
                ph: String::new(),
                bytes: start..end,
                morae: Vec::new(),
                groups: Vec::new(),
            });
        }
        start = end;
    }
}

/// Project an old block into one edited block or both Return halves. The explicit
/// edit range is in ORIGINAL Unicode scalars; None derives one conservative edit
/// interval by common prefix/suffix. Only annotations intersecting changed text or
/// a new sentence boundary are removed; unaffected ranges are preserved exactly.
pub(crate) fn project(
    before: &Block,
    targets: &mut [&mut Block],
    edit: Option<Range<usize>>,
) -> Result<(), String> {
    if before.sentences.iter().all(|s| s.ssml.is_none()) {
        return Ok(());
    }
    let original = &before.text;
    let updated: String = targets.iter().map(|block| block.text.as_str()).collect();
    let old = boundaries(original);
    let new = boundaries(&updated);
    let (old_len, new_len) = (old.len() - 1, new.len() - 1);
    let (start, old_end, new_end) = if let Some(edit) = edit {
        if edit.start > edit.end || edit.end > old_len {
            return Err("Pending annotation edit range is outside its source".into());
        }
        let new_end = new_len
            .checked_sub(old_len - edit.end)
            .ok_or("Pending edit length")?;
        if new_end < edit.start
            || original[..old[edit.start]] != updated[..new[edit.start]]
            || original[old[edit.end]..] != updated[new[new_end]..]
        {
            return Err("Pending annotation edit projection differs from text".into());
        }
        (edit.start, edit.end, new_end)
    } else {
        let left = original
            .chars()
            .zip(updated.chars())
            .take_while(|(a, b)| a == b)
            .count();
        let right = original
            .chars()
            .rev()
            .zip(updated.chars().rev())
            .take(old_len.min(new_len) - left)
            .take_while(|(a, b)| a == b)
            .count();
        (left, old_len - right, new_len - right)
    };
    let mut preserved = Vec::new();
    let mut source_offset = 0;
    for sentence in &before.sentences {
        if let Some(pending) = &sentence.ssml {
            pending.validate(&sentence.text)?;
            for annotation in &pending.annotations {
                let a = old
                    .binary_search(&(source_offset + annotation.bytes.start))
                    .map_err(|_| "Pending annotation starts inside a character")?;
                let b = old
                    .binary_search(&(source_offset + annotation.bytes.end))
                    .map_err(|_| "Pending annotation ends inside a character")?;
                let mapped = if b <= start {
                    Some(a..b)
                } else if a >= old_end {
                    Some((new_end + a - old_end)..(new_end + b - old_end))
                } else {
                    None
                };
                if let Some(range) = mapped {
                    preserved.push((new[range.start]..new[range.end], annotation.clone()));
                }
            }
        }
        source_offset += sentence.text.len();
    }
    distribute(targets, preserved)
}

fn distribute(
    targets: &mut [&mut Block],
    mut preserved: Vec<(Range<usize>, Annotation)>,
) -> Result<(), String> {
    preserved.sort_by_key(|(range, _)| (range.start, range.end));
    if preserved
        .windows(2)
        .any(|pair| pair[0].0.end > pair[1].0.start)
    {
        return Err("Overlapping pending annotations after edit".into());
    }
    let mut target_offset = 0;
    for block in targets {
        for sentence in &mut block.sentences {
            let end = target_offset + sentence.text.len();
            let mut annotations = Vec::new();
            let mut cursor = 0;
            for (range, original) in &preserved {
                if range.start < target_offset || range.end > end {
                    continue;
                }
                let local = (range.start - target_offset)..(range.end - target_offset);
                neutral(&sentence.text, cursor..local.start, &mut annotations);
                let mut annotation = original.clone();
                annotation.bytes = local.clone();
                annotations.push(annotation);
                cursor = local.end;
            }
            neutral(
                &sentence.text,
                cursor..sentence.text.len(),
                &mut annotations,
            );
            sentence.ssml = if annotations.iter().any(|a| !a.ph.is_empty()) {
                let pending = Pending {
                    source: sentence.text.clone(),
                    annotations,
                };
                pending.validate(&sentence.text)?;
                Some(pending)
            } else {
                None
            };
            target_offset = end;
        }
    }
    Ok(())
}

pub(crate) fn slice(pending: &Pending, selection: Range<usize>) -> Result<Option<Pending>, String> {
    pending.validate(&pending.source)?;
    let at = boundaries(&pending.source);
    if selection.start > selection.end || selection.end >= at.len() {
        return Err("Pending clipboard selection is outside text".into());
    }
    let bytes = at[selection.start]..at[selection.end];
    let mut block = Block::new(1);
    block.text = pending.source[bytes.clone()].to_owned();
    block.sentence_serial = 1;
    block.sentences = vec![crate::app::document::SentenceState {
        native_owner: None,
        id: 1,
        text: block.text.clone(),
        words: Vec::new(),
        word_positions: Default::default(),
        query: Default::default(),
        ssml: None,
    }];
    let preserved = pending
        .annotations
        .iter()
        .filter_map(|a| {
            (a.bytes.start >= bytes.start && a.bytes.end <= bytes.end).then(|| {
                (
                    (a.bytes.start - bytes.start)..(a.bytes.end - bytes.start),
                    a.clone(),
                )
            })
        })
        .collect();
    distribute(&mut [&mut block], preserved)?;
    Ok(block.sentences.into_iter().next().and_then(|s| s.ssml))
}

/// Incoming starts are UTF-8 offsets in the new block. Pending fragment annotations
/// already use offsets local to their fragment source; never use display-chunk offsets.
pub(crate) fn overlay(block: &mut Block, incoming: &[(usize, &Pending)]) -> Result<(), String> {
    let mut preserved = Vec::new();
    let mut offset = 0;
    for sentence in &block.sentences {
        if let Some(pending) = &sentence.ssml {
            pending.validate(&sentence.text)?;
            preserved.extend(
                pending
                    .annotations
                    .iter()
                    .map(|a| ((offset + a.bytes.start)..(offset + a.bytes.end), a.clone())),
            );
        }
        offset += sentence.text.len();
    }
    // Existing neutral gap records are rebuilt around authored incoming annotations.
    preserved.retain(|(_, a)| !a.ph.is_empty());
    for (start, pending) in incoming {
        pending.validate(&pending.source)?;
        let end = start
            .checked_add(pending.source.len())
            .ok_or("Pending paste size overflow")?;
        if block.text.get(*start..end) != Some(pending.source.as_str()) {
            return Err("Pending pasted text does not match its source".into());
        }
        preserved.extend(
            pending
                .annotations
                .iter()
                .filter(|a| !a.ph.is_empty())
                .map(|a| ((start + a.bytes.start)..(start + a.bytes.end), a.clone())),
        );
    }
    distribute(&mut [block], preserved)
}
