use crate::{Result, invalid};
use std::ops::Range;

#[derive(Clone)]
pub(super) struct Marked {
    pub(super) value: char,
    pub(super) origin: Range<usize>,
}

pub struct NormalizedText {
    pub text: String,
    origins: Vec<Range<usize>>,
    bytes: Vec<usize>,
}

impl NormalizedText {
    pub(super) fn new(chars: Vec<Marked>) -> Self {
        let mut text = String::new();
        let mut origins = Vec::with_capacity(chars.len());
        let mut bytes = Vec::with_capacity(chars.len() + 1);
        for ch in chars {
            bytes.push(text.len());
            text.push(ch.value);
            origins.push(ch.origin);
        }
        bytes.push(text.len());
        Self {
            text,
            origins,
            bytes,
        }
    }

    pub fn original_scalars(&self, normalized_bytes: Range<usize>) -> Result<Range<usize>> {
        let start = self
            .bytes
            .binary_search(&normalized_bytes.start)
            .map_err(|_| invalid("normalized word start is not a UTF-8 boundary"))?;
        let end = self
            .bytes
            .binary_search(&normalized_bytes.end)
            .map_err(|_| invalid("normalized word end is not a UTF-8 boundary"))?;
        if start >= end {
            return Err(invalid("normalized word range is empty or reversed"));
        }
        let mut range = self.origins[start].clone();
        for origin in &self.origins[start + 1..end] {
            range.start = range.start.min(origin.start);
            range.end = range.end.max(origin.end);
        }
        Ok(range)
    }
}

pub(super) const MAX_SCALARS: usize = 4_194_304;
