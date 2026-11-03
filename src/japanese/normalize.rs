//! Unicode sentence normalization before dictionary lookup.
use crate::{Result, invalid};
use std::collections::BTreeSet;

/// Call exactly once before morphological analysis.
pub fn normalize(input: &str) -> Result<String> {
    normalize_excluding(input, &BTreeSet::new())
}

/// Normalize with the raw-input exclusions supplied by the configured tagger.
pub fn normalize_excluding(input: &str, excluded: &BTreeSet<char>) -> Result<String> {
    let mut result = String::new();
    super::prefix::walk(input, excluded, |text, _| {
        if result.len().saturating_add(text.len()) > 1_048_576 {
            return Err(invalid("Japanese number input byte budget"));
        }
        result.push_str(text);
        Ok(())
    })?;
    super::numbers::expand(&result)
}

pub fn normalize_with_origins(input: &str) -> Result<super::origin::NormalizedText> {
    normalize_with_origins_excluding(input, &BTreeSet::new())
}

pub fn normalize_with_origins_excluding(
    input: &str,
    excluded: &BTreeSet<char>,
) -> Result<super::origin::NormalizedText> {
    use super::origin::{Marked, NormalizedText};
    let mut result = Vec::new();
    let mut prefix_bytes = 0usize;
    super::prefix::walk(input, excluded, |text, origin| {
        prefix_bytes = prefix_bytes.saturating_add(text.len());
        if prefix_bytes > 1_048_576 {
            return Err(invalid("Japanese number input byte budget"));
        }
        for value in text.chars() {
            if result.len() == super::origin::MAX_SCALARS {
                return Err(invalid("Normalization provenance expansion budget"));
            }
            result.push(Marked {
                value,
                origin: origin.clone(),
            });
        }
        Ok(())
    })?;
    let result = NormalizedText::new(super::numbers::expand_mapped(result)?);
    if result.text.len() > 4_194_304 {
        return Err(invalid("expanded sentence byte budget"));
    }
    Ok(result)
}
