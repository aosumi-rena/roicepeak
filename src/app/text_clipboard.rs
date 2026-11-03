use crate::app::sentence_edit::{
    Fragment, MAX_TEXT_BYTES, SourceSentence, ValidationBudget, WordPositions,
};
use serde::{Deserialize, Serialize};
use std::ops::Range;

pub(crate) const FORMAT_NAME: &str = "ROICEPEAK Sentence";
const VERSION: u32 = 2;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_SENTENCES: usize = 10_000;
#[cfg(windows)]
#[path = "text_clipboard_windows.rs"]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{read, write};

#[cfg(not(windows))]
pub(crate) fn write(ctx: &eframe::egui::Context, payload: &Payload) -> Result<(), String> {
    payload.validate()?;
    ctx.copy_text(payload.text());
    Ok(())
}

#[cfg(not(windows))]
pub(crate) fn read() -> Result<Option<Payload>, String> {
    Ok(None)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Payload {
    version: u32,
    pub sentences: Vec<Fragment>,
}

impl Payload {
    pub(crate) fn from_block(
        block: &crate::app::document::Block,
        selection: Range<usize>,
    ) -> Result<Option<Self>, String> {
        let sentences = block.resolved_sentences()?;
        let prepared: Vec<_> = sentences
            .iter()
            .map(|sentence| {
                sentence.word_positions.source == sentence.text
                    && sentence.word_positions.ranges.len() == sentence.words.len()
            })
            .collect();
        let positions: Vec<_> = sentences
            .iter()
            .zip(&prepared)
            .map(|(sentence, prepared)| {
                if *prepared {
                    sentence.word_positions.clone()
                } else {
                    WordPositions {
                        source: sentence.text.clone(),
                        ranges: Vec::new(),
                    }
                }
            })
            .collect();
        let sources = sentences
            .iter()
            .zip(&positions)
            .zip(&prepared)
            .map(|((sentence, positions), prepared)| {
                SourceSentence::new(
                    &sentence.text,
                    if *prepared { &sentence.words } else { &[] },
                    positions,
                    sentence.ssml.as_ref(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::slice(&sources, selection)
    }

    pub(crate) fn slice(
        sources: &[SourceSentence<'_>],
        selection: Range<usize>,
    ) -> Result<Option<Self>, String> {
        if sources.len() > MAX_SENTENCES {
            return Err("Clipboard sentence count limit".into());
        }
        let mut length = 0usize;
        let mut bytes = 0usize;
        for source in sources {
            length = length
                .checked_add(source.char_len())
                .ok_or("Text selection size overflow")?;
            bytes = bytes
                .checked_add(source.byte_len())
                .filter(|bytes| *bytes <= MAX_TEXT_BYTES)
                .ok_or("Clipboard source text limit")?;
        }
        if selection.start > selection.end || selection.end > length {
            return Err("Text selection is outside its block".into());
        }
        if selection.is_empty() {
            return Ok(None);
        }
        let mut sentences = Vec::new();
        let mut start = 0;
        for source in sources {
            let end = start + source.char_len();
            let lower = selection.start.max(start);
            let upper = selection.end.min(end);
            if lower < upper {
                if let Some(fragment) = source.slice(lower - start..upper - start)? {
                    sentences.push(fragment);
                }
            }
            start = end;
        }
        let payload = Self {
            version: VERSION,
            sentences,
        };
        payload.validate()?;
        Ok(Some(payload))
    }

    pub(crate) fn text(&self) -> String {
        self.sentences
            .iter()
            .map(|sentence| sentence.text.as_str())
            .collect()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.version != VERSION
            || self.sentences.is_empty()
            || self.sentences.len() > MAX_SENTENCES
        {
            return Err("Unsupported clipboard payload or sentence count".into());
        }
        let mut bytes = 0usize;
        let mut budget = ValidationBudget::default();
        for sentence in &self.sentences {
            bytes = bytes
                .checked_add(sentence.text.len())
                .filter(|bytes| *bytes <= MAX_TEXT_BYTES)
                .ok_or("Clipboard text limit")?;
            sentence.validate(&mut budget)?;
        }
        Ok(())
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_BYTES {
            return Err("Clipboard payload exceeds 16 MiB".into());
        }
        Ok(bytes)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES {
            return Err("Clipboard payload exceeds 16 MiB".into());
        }
        let payload: Self = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        payload.validate()?;
        Ok(payload)
    }
}
