//! Japanese prefix normalization with original-input provenance.
use crate::{Result, invalid};
use std::{collections::BTreeSet, ops::Range};

fn hangul(input: &str) -> Option<char> {
    let mut chars = input.chars();
    let leading = chars.next()? as u32;
    let vowel = chars.next()? as u32;
    if !(0x1100..=0x1112).contains(&leading) || !(0x1161..=0x1175).contains(&vowel) {
        return None;
    }
    let trailing = match chars.next() {
        None => 0,
        Some(ch) if (0x11a8..=0x11c2).contains(&(ch as u32)) => ch as u32 - 0x11a7,
        Some(_) => return None,
    };
    if chars.next().is_some() {
        return None;
    }
    char::from_u32(0xac00 + (leading - 0x1100) * 588 + (vowel - 0x1161) * 28 + trailing)
}

/// Visits replacements exactly once, in their original byte/scalar frame.
/// Every emitted replacement scalar owns the whole consumed original interval.
pub(super) fn walk(
    input: &str,
    excluded: &BTreeSet<char>,
    mut emit: impl FnMut(&str, Range<usize>) -> Result<()>,
) -> Result<()> {
    if input.len() > 1_048_576 || input.contains('\0') {
        return Err(invalid("Japanese sentence byte budget or embedded NUL"));
    }
    let mut byte = 0;
    let mut scalar = 0;
    while byte < input.len() {
        let mut consumed = None;
        let mut storage = [0; 4];
        for &width in super::prefix_data::PREFIX_BYTE_LENGTHS {
            let end = byte + width;
            if end > input.len() || !input.is_char_boundary(end) {
                continue;
            }
            let prefix = &input[byte..end];
            let replacement = if let Some(ch) = hangul(prefix) {
                ch.encode_utf8(&mut storage)
            } else if let Ok(index) =
                super::prefix_data::PREFIXES.binary_search_by_key(&prefix, |row| row.0)
            {
                super::prefix_data::PREFIXES[index].1
            } else {
                continue;
            };
            if prefix.chars().any(|ch| excluded.contains(&ch)) {
                // A matched but excluded prefix falls back to one original scalar.
                break;
            }
            let count = prefix.chars().count();
            emit(replacement, scalar..scalar + count)?;
            consumed = Some((end, count));
            break;
        }
        if let Some((end, count)) = consumed {
            byte = end;
            scalar += count;
        } else {
            let ch = input[byte..]
                .chars()
                .next()
                .ok_or_else(|| invalid("Japanese input boundary"))?;
            let end = byte + ch.len_utf8();
            emit(&input[byte..end], scalar..scalar + 1)?;
            byte = end;
            scalar += 1;
        }
    }
    Ok(())
}
