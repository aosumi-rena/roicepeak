//! Sentence ranges in the original block text.
use roicepeak::speech_frontend::Language;
use serde::{Deserialize, Serialize};
use std::ops::Range;

const DELIMITERS: [&str; 13] = [
    "........", ".......", "......", ".....", "....", "...", ". ", "\n", "。", "！", "?", "？",
    "\"",
];

const ENGLISH_DELIMITERS: [&str; 4] = ["!", "． ", ".　", "．　"];

pub(crate) fn ends_with_delimiter(text: &str, language: &str) -> bool {
    let english = language == "english";
    let ends = DELIMITERS
        .iter()
        .chain(extra_delimiters(language))
        .any(|delimiter| (!english || *delimiter != "\"") && text.ends_with(*delimiter));
    if ends || !english {
        return ends;
    }
    let trimmed = text.trim_end();
    let body = trimmed.trim_end_matches(english_closer);
    body.len() != trimmed.len()
        && body
            .trim_end()
            .ends_with(['.', '．', '。', '!', '！', '?', '？'])
}

fn english_closer(ch: char) -> bool {
    matches!(ch, '"' | '\'' | '”' | '’' | '»' | ')' | ']' | '}')
}

fn english_period(tail: &str) -> Option<usize> {
    let first = tail.chars().next()?;
    let end = first.len_utf8();
    (matches!(first, '.' | '．')
        && (tail.len() == end
            || tail[end..].starts_with(|ch: char| english_closer(ch) || ch.is_whitespace())))
    .then_some(end)
}

fn extra_delimiters(language: &str) -> &[&str] {
    if language == "english" {
        &ENGLISH_DELIMITERS
    } else {
        &[]
    }
}

/// Sentence range in the original block text.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SentenceRef {
    pub index: usize,
    pub chars: Range<usize>,
    pub bytes: Range<usize>,
}

pub fn partition(text: &str) -> Vec<SentenceRef> {
    partition_for_language(text, "japanese")
}

pub fn partition_for_language(text: &str, language: &str) -> Vec<SentenceRef> {
    let english = Language::from_name(language) == Some(Language::English);
    let mut units = Vec::new();
    let (mut byte, mut character) = (0, 0);
    let (mut start_byte, mut start_character) = (0, 0);
    let mut pending_delimiter = false;
    let extra = extra_delimiters(language);
    while byte < text.len() {
        let tail = &text[byte..];
        if english && pending_delimiter {
            let after = tail.trim_start_matches(english_closer);
            if after.len() != tail.len() {
                let consumed = tail.len() - after.len();
                byte += consumed;
                character += tail[..consumed].chars().count();
                pending_delimiter = !after
                    .trim_start_matches([' ', '\t', '\u{3000}', ','])
                    .starts_with(char::is_lowercase);
                continue;
            }
        }
        let delimiter = DELIMITERS
            .iter()
            .chain(extra)
            .find(|value| (!english || **value != "\"") && tail.starts_with(**value))
            .map(|value| value.len())
            .or_else(|| english.then(|| english_period(tail)).flatten());
        if delimiter.is_none() && pending_delimiter {
            units.push(SentenceRef {
                index: units.len(),
                chars: start_character..character,
                bytes: start_byte..byte,
            });
            (start_byte, start_character) = (byte, character);
            pending_delimiter = false;
        }
        let english_word = english && !text[..byte].ends_with(char::is_alphanumeric);
        if let Some(len) = english_word
            .then(|| Language::English.honorific_len(tail))
            .flatten()
        {
            byte += len;
            character += tail[..len].chars().count();
        } else if let Some(len) = english_word
            .then(|| Language::English.initialism_len(tail))
            .flatten()
        {
            let after = &tail[len..];
            let spaced = after.trim_start_matches([' ', '\t', '\u{3000}']);
            pending_delimiter =
                spaced.len() != after.len() && spaced.starts_with(|ch: char| ch.is_uppercase());
            let consumed = if pending_delimiter {
                len + after.len() - spaced.len()
            } else {
                len
            };
            byte += consumed;
            character += tail[..consumed].chars().count();
        } else if let Some(len) = delimiter {
            byte += len;
            character += tail[..len].chars().count();
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
