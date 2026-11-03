use crate::app::document::state::{DurationMode, DurationState};
use eframe::egui::{TextBuffer, text::CharIndex};
use std::ops::Range;

pub(super) struct Buffer<'a>(pub(super) &'a mut String);

fn accepted(c: char) -> bool {
    c.is_ascii_digit() || matches!(c, '+' | '-' | '.')
}

impl TextBuffer for Buffer<'_> {
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<Buffer<'static>>()
    }
    fn is_mutable(&self) -> bool {
        true
    }
    fn as_str(&self) -> &str {
        self.0.as_str()
    }
    fn insert_text(&mut self, text: &str, at: CharIndex) -> usize {
        let remaining = 12usize.saturating_sub(self.0.chars().count());
        let filtered: String = text
            .chars()
            .filter(|c| accepted(*c))
            .take(remaining)
            .collect();
        TextBuffer::insert_text(self.0, &filtered, at)
    }
    fn delete_char_range(&mut self, range: Range<CharIndex>) {
        TextBuffer::delete_char_range(self.0, range);
    }
}

fn decimal(text: &str) -> Option<f64> {
    if text.len() > 12 || !text.chars().all(accepted) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut at = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let negative = bytes.first() == Some(&b'-');
    let integer_start = at;
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    let integer = text[integer_start..at].trim_start_matches('0');
    let fraction = if bytes.get(at) == Some(&b'.') {
        at += 1;
        let start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        Some(&text[start..at])
    } else {
        None
    };
    if integer.is_empty() && fraction.is_none_or(str::is_empty) {
        return Some(0.0);
    }
    let mut normalized = String::with_capacity(13);
    if negative {
        normalized.push('-');
    }
    normalized.push_str(integer);
    if let Some(fraction) = fraction {
        normalized.push('.');
        normalized.push_str(fraction);
    }
    normalized
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

pub(super) fn duration(text: &str, mode: DurationMode) -> Option<DurationState> {
    let parsed = decimal(text)?;
    Some(DurationState {
        mode,
        value: match mode {
            DurationMode::Relative => (parsed / 100.0 + 1.0).clamp(0.5, 2.0),
            DurationMode::Absolute => (parsed / 1000.0).clamp(0.01, 10.0),
        },
    })
}
