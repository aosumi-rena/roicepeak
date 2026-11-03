//! Preserve raw spellings found in explicit user dictionaries during normalization.
use super::{Tagger, UserDictionary};
use crate::{Result, invalid};
use std::collections::BTreeSet;

impl Tagger {
    /// Find all raw user-dictionary prefixes at every original scalar boundary.
    /// Every scalar in any match is excluded throughout this one sentence.
    /// System/unknown dictionaries, token costs and selected lattice paths do
    /// not contribute to this set.
    pub fn normalization_exclusions(
        &self,
        original: &str,
        user: Option<&UserDictionary>,
    ) -> Result<BTreeSet<char>> {
        if original.len() > 1_048_576 || original.contains('\0') {
            return Err(invalid("normalization dictionary input budget or NUL"));
        }
        if user.is_some_and(|user| user.directory() != self.directory.as_path()) {
            return Err(invalid("user entries belong to a different dictionary"));
        }
        let mut excluded = BTreeSet::new();
        let mut steps = 0usize;
        for (start, _) in original.char_indices() {
            let suffix = &original[start..];
            for dictionary in self.lexicons.iter().skip(1) {
                for (_, length) in dictionary.prefixes(suffix.as_bytes(), &mut steps)? {
                    if length != 0 {
                        let matched = suffix.get(..length).ok_or_else(|| {
                            invalid("user dictionary prefix is not a UTF-8 boundary")
                        })?;
                        for scalar in matched.chars() {
                            super::lookup_step(&mut steps)?;
                            excluded.insert(scalar);
                        }
                    }
                }
            }
            if let Some(user) = user {
                for index in user.prefixes(suffix, &mut steps)? {
                    let length = user.entry(index).spelling.len();
                    let matched = suffix
                        .get(..length)
                        .ok_or_else(|| invalid("user dictionary prefix is not a UTF-8 boundary"))?;
                    for scalar in matched.chars() {
                        super::lookup_step(&mut steps)?;
                        excluded.insert(scalar);
                    }
                }
            }
        }
        Ok(excluded)
    }
}
