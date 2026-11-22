use crate::app::document::WordState;
use serde::{Deserialize, Serialize};
use std::ops::Range;

mod richpaste;
pub(crate) use richpaste::replace_rich;

pub(crate) const MAX_TEXT_BYTES: usize = 1_000_000;
pub(crate) const MAX_WORDS: usize = 4096;
pub(crate) const MAX_PHONES: usize = 100_000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WordPositions {
    pub source: String,
    pub ranges: Vec<Option<Range<usize>>>,
    pub raw_ranges: Vec<Option<[i32; 2]>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PositionedWord {
    pub range: Range<usize>,
    pub raw_range: Option<[i32; 2]>,
    pub state: WordState,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fragment {
    pub text: String,
    pub completes_sentence: bool,
    pub words: Vec<PositionedWord>,
    #[serde(deserialize_with = "crate::app::document::ssml::required")]
    pub ssml: Option<crate::app::document::ssml::Pending>,
}

pub(crate) struct SourceSentence<'a> {
    text: &'a str,
    words: &'a [WordState],
    raw_ranges: Vec<Option<[i32; 2]>>,
    pending: Option<&'a crate::app::document::ssml::Pending>,
    boundaries: Vec<usize>,
}

fn boundary(word: &WordState) -> bool {
    matches!(word.pos, 0x2000 | 0x2001)
}

fn text_boundaries(text: &str) -> Result<Vec<usize>, String> {
    if text.len() > MAX_TEXT_BYTES || text.contains('\0') {
        return Err("Sentence text exceeds its limit or contains NUL".into());
    }
    Ok(text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .collect())
}

fn valid_range(range: &Range<usize>, length: usize) -> bool {
    range.start < range.end && range.end <= length
}

#[derive(Default)]
pub(crate) struct ValidationBudget {
    words: usize,
    moras: usize,
    phones: usize,
}

pub(crate) fn validate_word(word: &WordState, budget: &mut ValidationBudget) -> Result<(), String> {
    budget.words = budget
        .words
        .checked_add(1)
        .filter(|count| *count <= MAX_WORDS)
        .ok_or("Clipboard word count limit")?;
    budget.moras = budget
        .moras
        .checked_add(word.morae.len())
        .filter(|count| *count <= MAX_PHONES)
        .ok_or("Clipboard mora count limit")?;
    if word.surface.len() > MAX_TEXT_BYTES
        || word.surface.contains('\0')
        || !(1..=3).contains(&word.language)
        || word.morae.len() > MAX_PHONES
    {
        return Err("Invalid clipboard word".into());
    }
    for mora in &word.morae {
        if mora.reading.len() > 4096
            || mora.reading.contains('\0')
            || !mora.intonation_delta.is_finite()
            || !roicepeak::prosody::CODES.contains(&mora.code)
        {
            return Err("Invalid clipboard mora".into());
        }
        budget.phones = budget
            .phones
            .checked_add(mora.phones.len())
            .filter(|count| *count <= MAX_PHONES)
            .ok_or("Clipboard phone count limit")?;
        if mora.phones.iter().any(|phone| {
            phone.symbol.len() > 4096 || phone.symbol.contains('\0') || !phone.duration.valid()
        }) {
            return Err("Invalid clipboard phone".into());
        }
    }
    Ok(())
}

impl WordPositions {
    pub(crate) fn from_analysis(
        text: &str,
        analysis: &roicepeak::japanese::Analysis,
    ) -> Result<Self, String> {
        if analysis.input_ranges.len() != analysis.tokens.len() {
            return Err("Analysis word positions do not match its tokens".into());
        }
        if analysis.input_text.as_deref() == Some(text) {
            let boundaries = text_boundaries(text)?;
            let scalar = |offset| {
                boundaries
                    .binary_search(&offset)
                    .map_err(|_| "Analysis word position is not a character boundary".to_owned())
            };
            let ranges = analysis
                .input_ranges
                .iter()
                .map(|range| {
                    range
                        .as_ref()
                        .map(|range| Ok(scalar(range.start)?..scalar(range.end)?))
                        .transpose()
                })
                .collect::<Result<Vec<_>, String>>()?;
            let raw_ranges = signed_ranges(&ranges)?;
            return Ok(Self {
                source: text.to_owned(),
                ranges,
                raw_ranges,
            });
        }
        let normalized = roicepeak::japanese::normalize_with_origins_excluding(
            text,
            &analysis.normalization_exclusions,
        )
        .map_err(|e| e.to_string())?;
        if analysis.input_text.as_deref() != Some(normalized.text.as_str()) {
            return Err("Analysis word positions belong to different normalized text".into());
        }
        let ranges = analysis
            .input_ranges
            .iter()
            .map(|range| {
                range
                    .clone()
                    .map(|range| {
                        normalized
                            .original_scalars(range)
                            .map_err(|e| e.to_string())
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let raw_ranges = signed_ranges(&ranges)?;
        Ok(Self {
            source: text.to_owned(),
            ranges,
            raw_ranges,
        })
    }

    pub(crate) fn stored_ranges(&self, word_count: usize) -> Result<Vec<Option<[i32; 2]>>, String> {
        if !self.raw_ranges.is_empty() {
            if self.raw_ranges.len() != word_count {
                return Err("Stored word positions do not match their words".into());
            }
            return Ok(self.raw_ranges.clone());
        }
        if self.ranges.len() == word_count {
            return signed_ranges(&self.ranges);
        }
        if self.ranges.is_empty() {
            return Ok(vec![None; word_count]);
        }
        Err("Word positions do not match their words".into())
    }

    pub(crate) fn validate(&self, words: &[WordState]) -> Result<(), String> {
        if !self.raw_ranges.is_empty() && self.raw_ranges.len() != words.len() {
            return Err("Stored word positions do not match their words".into());
        }
        if self.source.is_empty() && self.ranges.is_empty() {
            return Ok(());
        }
        let length = text_boundaries(&self.source)?.len() - 1;
        if words.len() > MAX_WORDS || self.ranges.len() != words.len() {
            return Err("Word positions do not match their canonical words".into());
        }
        for (word, range) in words.iter().zip(&self.ranges) {
            if boundary(word) {
                if range.is_some() {
                    return Err("Synthetic boundaries cannot own original text".into());
                }
            } else if range
                .as_ref()
                .is_some_and(|range| !valid_range(range, length))
            {
                return Err("Word position is outside its original text".into());
            }
        }
        Ok(())
    }
}

impl<'a> SourceSentence<'a> {
    pub(crate) fn new(
        text: &'a str,
        words: &'a [WordState],
        positions: &'a WordPositions,
        pending: Option<&'a crate::app::document::ssml::Pending>,
    ) -> Result<Self, String> {
        positions.validate(words)?;
        if let Some(pending) = pending {
            pending.validate(text)?;
        }
        let raw_ranges = positions.stored_ranges(words.len())?;
        let mut budget = ValidationBudget::default();
        for word in words {
            validate_word(word, &mut budget)?;
        }
        Ok(Self {
            text,
            words,
            raw_ranges,
            pending,
            boundaries: text_boundaries(text)?,
        })
    }

    pub(crate) fn char_len(&self) -> usize {
        self.boundaries.len() - 1
    }

    pub(crate) fn byte_len(&self) -> usize {
        self.text.len()
    }

    pub(crate) fn slice(&self, selection: Range<usize>) -> Result<Option<Fragment>, String> {
        if selection.start > selection.end || selection.end > self.char_len() {
            return Err("Text selection is outside its sentence".into());
        }
        if selection.is_empty() {
            return Ok(None);
        }
        let text =
            self.text[self.boundaries[selection.start]..self.boundaries[selection.end]].to_owned();
        let mut words = Vec::new();
        for (word, raw) in self.words.iter().zip(&self.raw_ranges) {
            let Some([start, end]) = *raw else {
                continue;
            };
            if i64::from(start) < selection.start as i64 || i64::from(end) > selection.end as i64 {
                continue;
            }
            if end <= start {
                return Err("Copied word has no nonempty text range".into());
            }
            words.push(PositionedWord {
                range: start as usize - selection.start..end as usize - selection.start,
                raw_range: *raw,
                state: word.clone(),
            });
        }
        Ok(Some(Fragment {
            text,
            completes_sentence: selection.end == self.char_len(),
            words,
            ssml: self
                .pending
                .map(|p| crate::app::document::ssml::slice(p, selection.clone()))
                .transpose()?
                .flatten(),
        }))
    }
}

impl Fragment {
    pub(crate) fn validate(&self, budget: &mut ValidationBudget) -> Result<(), String> {
        let length = text_boundaries(&self.text)?.len() - 1;
        if length == 0 || self.words.len() > MAX_WORDS {
            return Err("Invalid clipboard sentence".into());
        }
        if let Some(pending) = &self.ssml {
            pending.validate(&self.text)?;
        }
        for word in &self.words {
            if boundary(&word.state) || !valid_range(&word.range, length) {
                return Err("Invalid clipboard word position".into());
            }
            validate_word(&word.state, budget)?;
        }
        Ok(())
    }
}

pub(crate) fn replace(
    block: &crate::app::document::Block,
    selection: Range<usize>,
    inserted: &str,
) -> Result<crate::app::document::Block, String> {
    text_boundaries(inserted)?;
    let boundaries = text_boundaries(&block.text)?;
    if selection.start > selection.end || selection.end >= boundaries.len() {
        return Err("Text selection is outside its block".into());
    }
    let replaced = boundaries[selection.end] - boundaries[selection.start];
    if (block.text.len() - replaced)
        .checked_add(inserted.len())
        .is_none_or(|length| length > MAX_TEXT_BYTES)
    {
        return Err("Edited block exceeds its text limit".into());
    }
    let mut staged = block.clone();
    staged.sync_sentences()?;
    let before = staged.clone();
    staged.text.replace_range(
        boundaries[selection.start]..boundaries[selection.end],
        inserted,
    );
    if staged.text != before.text {
        staged.sync_partition()?;
        crate::app::document::ssml::project(&before, &mut [&mut staged], Some(selection))?;
        staged.segments.clear();
    }
    staged.validate_sentences()?;
    Ok(staged)
}

fn signed_ranges(ranges: &[Option<Range<usize>>]) -> Result<Vec<Option<[i32; 2]>>, String> {
    ranges
        .iter()
        .map(|range| {
            range
                .as_ref()
                .map(|range| {
                    Ok([
                        i32::try_from(range.start).map_err(|_| "Word position exceeds int32")?,
                        i32::try_from(range.end).map_err(|_| "Word position exceeds int32")?,
                    ])
                })
                .transpose()
        })
        .collect()
}
