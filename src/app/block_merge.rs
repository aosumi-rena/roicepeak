//! Authored adjacent-block merge; derived cells are rebuilt by preparation.
use super::{Block, state::WordQueryState};
use crate::app::sentence_edit::WordPositions;
use std::collections::BTreeSet;

fn positions(sentence: &super::SentenceState) -> Vec<Option<std::ops::Range<usize>>> {
    if sentence.word_positions.source == sentence.text
        && sentence.word_positions.ranges.len() == sentence.words.len()
    {
        sentence.word_positions.ranges.clone()
    } else {
        vec![None; sentence.words.len()]
    }
}

impl Block {
    /// Keep the left block's identity, voice, parameters and placement. Whole
    /// sentences move intact; only an unterminated boundary sentence is joined.
    pub(crate) fn merged_with(&self, right: &Block) -> Result<Block, String> {
        self.text
            .len()
            .checked_add(right.text.len())
            .filter(|size| *size <= crate::app::sentence_edit::MAX_TEXT_BYTES)
            .ok_or("Merged block exceeds its text limit")?;
        let mut merged = self.clone();
        merged.segments.clear();
        merged.sync_sentences()?;
        let mut incoming = right.resolved_sentences()?;
        let mut authored_pending = Vec::new();
        let left_bytes = merged.text.len();
        if !incoming.is_empty()
            && merged
                .sentences
                .last()
                .is_some_and(|sentence| !crate::app::sentences::ends_with_delimiter(&sentence.text))
        {
            let first = incoming.remove(0);
            let last = merged
                .sentences
                .last_mut()
                .ok_or("Merge boundary disappeared")?;
            let old_text = last.text.clone();
            let old_scalars = old_text.chars().count();
            let mut ranges = positions(last);
            ranges.extend(
                positions(&first)
                    .into_iter()
                    .map(|range| range.map(|r| r.start + old_scalars..r.end + old_scalars)),
            );
            if let Some(pending) = last.ssml.take() {
                authored_pending.push((left_bytes - old_text.len(), pending));
            }
            if let Some(pending) = first.ssml {
                authored_pending.push((left_bytes, pending));
            }
            last.text.push_str(&first.text);
            last.words.extend(first.words);
            last.word_positions = WordPositions {
                source: last.text.clone(),
                ranges,
            };
            // These snapshots describe two prior query inputs. Keep the authored
            // words, but do not expose either old query as the joined query.
            last.query = WordQueryState::default();
        }
        let mut ids: BTreeSet<_> = merged
            .sentences
            .iter()
            .map(|sentence| sentence.id)
            .collect();
        let mut serial = merged.sentence_serial.max(right.sentence_serial);
        for sentence in &mut incoming {
            // Native sentence identities are global; own identities are local to
            // each block. Remap collisions without changing a surviving left ID.
            if !ids.insert(sentence.id) {
                serial = serial.checked_add(1).ok_or("Sentence identity exhausted")?;
                sentence.id = serial;
                ids.insert(serial);
            }
        }
        merged.sentence_serial = serial;
        merged.sentences.extend(incoming);
        merged.text = merged
            .sentences
            .iter()
            .map(|sentence| sentence.text.as_str())
            .collect();
        if merged.text.len() != left_bytes + right.text.len() {
            return Err("Merged sentence text differs from its source blocks".into());
        }
        if !authored_pending.is_empty() {
            let incoming = authored_pending
                .iter()
                .map(|(start, pending)| (*start, pending))
                .collect::<Vec<_>>();
            super::ssml::overlay(&mut merged, &incoming)?;
        }
        merged.validate_sentences()?;
        Ok(merged)
    }
}
