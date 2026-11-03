//! Sentence ranges in the original block text.
use serde::{Deserialize, Serialize};
use std::ops::Range;

const DELIMITERS: [&str; 13] = [
    "........", ".......", "......", ".....", "....", "...", ". ", "\n", "。", "！", "?", "？",
    "\"",
];

pub(crate) fn ends_with_delimiter(text: &str) -> bool {
    DELIMITERS
        .iter()
        .any(|delimiter| text.ends_with(*delimiter))
}

/// A sentence address in the unnormalized block text. Token indices are local
/// to this address; normalization does not imply token-to-source alignment.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SentenceRef {
    pub index: usize,
    pub chars: Range<usize>,
    pub bytes: Range<usize>,
}

/// Keep a consecutive delimiter run in the preceding unit. The first following
/// non-delimiter starts the next unit; leading delimiters may form a unit.
pub fn partition(text: &str) -> Vec<SentenceRef> {
    let mut units = Vec::new();
    let (mut byte, mut character) = (0, 0);
    let (mut start_byte, mut start_character) = (0, 0);
    let mut pending_delimiter = false;
    while byte < text.len() {
        let tail = &text[byte..];
        let delimiter = DELIMITERS.iter().find(|value| tail.starts_with(**value));
        if delimiter.is_none() && pending_delimiter {
            units.push(SentenceRef {
                index: units.len(),
                chars: start_character..character,
                bytes: start_byte..byte,
            });
            (start_byte, start_character) = (byte, character);
            pending_delimiter = false;
        }
        if let Some(delimiter) = delimiter {
            byte += delimiter.len();
            character += delimiter.chars().count();
            pending_delimiter = true;
        } else {
            byte += tail.chars().next().unwrap().len_utf8();
            character += 1;
        }
    }
    if start_byte < byte {
        units.push(SentenceRef {
            index: units.len(),
            chars: start_character..character,
            bytes: start_byte..byte,
        });
    }
    units
}
