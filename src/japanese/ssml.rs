//! Kana readings embedded in SSML phoneme annotations.
use super::reading::{normalize_char, symbols, vowel};
use crate::{
    Result, invalid,
    phonemes::{LOW, Phone, Syllable},
};
use std::ops::Range;

const MAX_BYTES: usize = 1_048_576;
const MAX_MORAE: usize = 100_000;
const MAX_GROUPS: usize = 100_000;
const MAX_PHONES: usize = 300_000;
const MAX_READING_BYTES: usize = 4096;

#[derive(Default)]
pub struct SsmlReading {
    pub morae: Vec<SsmlMora>,
    pub groups: Vec<Range<usize>>,
}
pub struct SsmlMora {
    pub syllable: Syllable,
    pub prediction_eligible: bool,
    pub accent_edited: bool,
}

pub fn ssml_reading(text: &str) -> Result<SsmlReading> {
    if text.len() > MAX_BYTES || text.contains('\0') {
        return Err(invalid("SSML reading byte budget or embedded NUL"));
    }
    let normalized = text
        .replace("\u{30ab}\u{309a}", "\u{30ac}")
        .replace("\u{30ad}\u{309a}", "\u{30ae}")
        .replace("\u{30af}\u{309a}", "\u{30b0}")
        .replace("\u{30b1}\u{309a}", "\u{30b2}")
        .replace("\u{30b3}\u{309a}", "\u{30b4}");
    let filtered: String = normalized
        .chars()
        .map(normalize_char)
        .filter(|&c| vowel(c).is_some() || matches!(c, '\u{30fc}' | '\u{30fb}' | '[' | ']'))
        .collect();
    let mut result = SsmlReading::default();
    if filtered.is_empty() {
        return Ok(result);
    }
    let mut phone_count = 0usize;
    for group in filtered.split('\u{30fb}') {
        if result.groups.len() >= MAX_GROUPS {
            return Err(invalid("SSML reading group budget"));
        }
        let start = result.morae.len();
        let open = group.contains('[');
        let close = group.contains(']');
        let manual = open || close;
        let mut high = close && !open;
        let mut previous = '\0';
        let mut buffer = String::new();
        for mut c in group.chars() {
            let small = matches!(
                c,
                '\u{30a1}'
                    | '\u{30a3}'
                    | '\u{30a5}'
                    | '\u{30a7}'
                    | '\u{30a9}'
                    | '\u{30e3}'
                    | '\u{30e5}'
                    | '\u{30e7}'
                    | '\u{30ee}'
            );
            if !small {
                flush(
                    &mut result.morae,
                    &mut buffer,
                    high,
                    manual,
                    &mut phone_count,
                )?;
            }
            match c {
                '[' => {
                    high = true;
                    continue;
                }
                ']' => {
                    high = false;
                    continue;
                }
                '\u{30fc}' => c = vowel(previous).unwrap_or(c),
                _ => {}
            }
            if buffer.len() + c.len_utf8() > MAX_READING_BYTES {
                return Err(invalid("SSML mora reading byte budget"));
            }
            buffer.push(c);
            previous = c;
        }
        flush(
            &mut result.morae,
            &mut buffer,
            high,
            manual,
            &mut phone_count,
        )?;
        result.groups.push(start..result.morae.len());
    }
    Ok(result)
}

fn flush(
    morae: &mut Vec<SsmlMora>,
    buffer: &mut String,
    high: bool,
    manual: bool,
    phone_count: &mut usize,
) -> Result<()> {
    if buffer.is_empty() {
        return Ok(());
    }
    if morae.len() >= MAX_MORAE {
        return Err(invalid("SSML reading mora budget"));
    }
    let reading = std::mem::take(buffer);
    let mut phones = Vec::new();
    for symbol in symbols(&reading).unwrap_or("").split_whitespace() {
        *phone_count = phone_count
            .checked_add(1)
            .filter(|count| *count <= MAX_PHONES)
            .ok_or_else(|| invalid("SSML reading phone budget"))?;
        phones.push(Phone {
            symbol: symbol.to_owned(),
            nucleus: false,
        });
    }
    morae.push(SsmlMora {
        syllable: Syllable {
            reading,
            code: LOW + u32::from(high),
            phones,
        },
        prediction_eligible: true,
        accent_edited: manual,
    });
    Ok(())
}
