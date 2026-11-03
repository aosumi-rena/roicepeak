//! Pending source-bound SSML phoneme annotations.
use super::{
    Block, WordState,
    state::{DurationState, MoraState, PhoneState},
};
use crate::app::sentence_edit::{ValidationBudget, WordPositions, validate_word};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, ops::Range};
#[path = "ssml_pending_edit.rs"]
mod edit;
pub(crate) use edit::{overlay, project, slice};

const MAX_ANNOTATIONS: usize = 100_000;
const MAX_TEXT: usize = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Annotation {
    pub surface: String,
    pub ph: String,
    pub bytes: Range<usize>,
    pub morae: Vec<MoraState>,
    pub groups: Vec<Range<usize>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pending {
    pub source: String,
    pub annotations: Vec<Annotation>,
}

pub(crate) struct Unassigned {
    pub surface: String,
    pub ph: String,
    pub morae: Vec<MoraState>,
    pub groups: Vec<Range<usize>>,
}

// A missing field must be rejected rather than interpreted as an absent queue.
// deserialize_with bypasses serde's implicit missing-Option default.
pub(crate) fn required<'de, D>(deserializer: D) -> Result<Option<Pending>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<Pending>::deserialize(deserializer)
}

pub(crate) fn from_reading(
    reading: roicepeak::japanese::SsmlReading,
) -> (Vec<MoraState>, Vec<Range<usize>>) {
    let morae = reading
        .morae
        .into_iter()
        .map(|m| MoraState {
            reading: m.syllable.reading,
            code: m.syllable.code,
            prediction_eligible: m.prediction_eligible,
            accent_edited: m.accent_edited,
            intonation_delta: 0.0,
            // Source is the Japanese SAPI-kana phone sequence. Retained target word
            // language/POS do not change this source classification.
            phones: m
                .syllable
                .phones
                .into_iter()
                .map(|p| {
                    let nucleus = roicepeak::japanese::is_nucleus(&p.symbol);
                    PhoneState {
                        symbol: p.symbol,
                        nucleus,
                        duration: DurationState::default(),
                    }
                })
                .collect(),
        })
        .collect();
    (morae, reading.groups)
}

impl Pending {
    pub(crate) fn validate(&self, text: &str) -> Result<(), String> {
        if self.source != text
            || text.len() > MAX_TEXT
            || text.contains('\0')
            || self.annotations.is_empty()
            || self.annotations.len() > MAX_ANNOTATIONS
        {
            return Err("Pending SSML annotations do not match their sentence".into());
        }
        let mut budget = ValidationBudget::default();
        let mut end = 0;
        for a in &self.annotations {
            if a.surface.len() > MAX_TEXT
                || a.ph.len() > MAX_TEXT
                || a.surface.contains('\0')
                || a.ph.contains('\0')
                || a.bytes.start < end
                || a.bytes.start > a.bytes.end
                || text.get(a.bytes.clone()) != Some(a.surface.replace(' ', "").as_str())
                || a.groups.len() > MAX_ANNOTATIONS
                || a.groups
                    .iter()
                    .any(|g| g.start > g.end || g.end > a.morae.len())
            {
                return Err("Invalid pending SSML annotation".into());
            }
            validate_word(
                &WordState {
                    surface: a.surface.clone(),
                    language: 1,
                    pos: 0,
                    pronunciation_edited: true,
                    morae: a.morae.clone(),
                },
                &mut budget,
            )?;
            end = a.bytes.end;
        }
        Ok(())
    }
}

/// Assign an ordered flush queue using the original sentence UTF-8 bytes.
/// No surface searching, forward skipping, tokenization or dictionary is involved.
pub(crate) fn assign(block: &mut Block, chunks: Vec<Unassigned>) -> Result<usize, String> {
    if chunks.len() > MAX_ANNOTATIONS {
        return Err("SSML annotation count limit".into());
    }
    let mut queue: VecDeque<_> = chunks.into();
    for sentence in &mut block.sentences {
        let mut cursor = 0;
        let mut annotations = Vec::new();
        while let Some(first) = queue.front() {
            while sentence.text.as_bytes().get(cursor) == Some(&b' ') {
                cursor += 1;
            }
            let normalized = first.surface.replace(' ', "");
            let Some(end) = cursor.checked_add(normalized.len()) else {
                return Err("SSML annotation byte overflow".into());
            };
            if sentence.text.get(cursor..end) != Some(normalized.as_str()) {
                break;
            }
            let first = queue.pop_front().ok_or("SSML queue disappeared")?;
            annotations.push(Annotation {
                surface: first.surface,
                ph: first.ph,
                bytes: cursor..end,
                morae: first.morae,
                groups: first.groups,
            });
            cursor = end;
        }
        sentence.ssml = if annotations.is_empty() {
            None
        } else {
            let pending = Pending {
                source: sentence.text.clone(),
                annotations,
            };
            pending.validate(&sentence.text)?;
            Some(pending)
        };
    }
    // Native flush destroys annotations that could not be matched to a sentence.
    Ok(queue.len())
}

/// Apply to raw word order, retaining word surfaces, language, POS and positions.
/// Synthetic BOS/EOS have no source position and never enter the remaining queue.
pub(crate) fn apply(
    pending: &Pending,
    positions: &WordPositions,
    words: &mut [WordState],
) -> Result<(), String> {
    pending.validate(&positions.source)?;
    positions.validate(words)?;
    let mut boundaries: Vec<_> = positions.source.char_indices().map(|(i, _)| i).collect();
    boundaries.push(positions.source.len());
    let mut remaining = VecDeque::new();
    for (index, range) in positions.ranges.iter().enumerate() {
        if let Some(range) = range {
            let start = *boundaries
                .get(range.start)
                .ok_or("SSML word start outside text")?;
            let end = *boundaries
                .get(range.end)
                .ok_or("SSML word end outside text")?;
            remaining.push_back((index, start..end));
        }
    }
    for annotation in &pending.annotations {
        let mut contained = Vec::new();
        while let Some((index, range)) = remaining.front() {
            if annotation.bytes.start > range.start || range.end > annotation.bytes.end {
                break;
            }
            contained.push(*index);
            remaining.pop_front();
        }
        if annotation.ph.is_empty() {
            continue;
        }
        for (ordinal, index) in contained.into_iter().enumerate() {
            words[index].morae = if ordinal == 0 {
                annotation.morae.clone()
            } else {
                Vec::new()
            };
            words[index].pronunciation_edited = true;
        }
    }
    Ok(())
}
