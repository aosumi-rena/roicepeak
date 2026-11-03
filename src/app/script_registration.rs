//! Prepare an editable dictionary draft from an original scalar selection.
use crate::app::{dictionary::Entry, document::Block, text_clipboard::Payload};
use std::ops::Range;

pub(crate) fn entry(block: &Block, selection: Range<usize>) -> Result<Option<Entry>, String> {
    let Some(payload) = Payload::from_block(block, selection)? else {
        return Ok(None);
    };
    let mut entry = Entry::default();
    entry.word = payload.text();
    if entry.word.len() > 4096 {
        return Err("Selected dictionary spelling exceeds its text limit".into());
    }
    let canonical: String = payload
        .sentences
        .iter()
        .flat_map(|sentence| &sentence.words)
        .map(|word| word.state.surface.as_str())
        .collect();
    if canonical == entry.word {
        for word in payload
            .sentences
            .iter()
            .flat_map(|sentence| &sentence.words)
        {
            for mora in &word.state.morae {
                entry.reading.push_str(&mora.reading);
                entry.native.overwrite_accents.push(mora.code as i32);
            }
        }
        if entry.reading.len() > 4096 || entry.native.overwrite_accents.len() > 4096 {
            return Err("Selected dictionary reading exceeds its limit".into());
        }
    }
    // Native51F344 initializes accentType0 and51F5B1 copies that unchanged;
    // selected codes are overwriteAccents, not an inferred accent-type number.
    Ok(Some(entry))
}
