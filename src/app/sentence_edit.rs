use crate::app::document::WordState;
use serde::{Deserialize, Serialize};
use std::ops::Range;

pub(crate) const MAX_TEXT_BYTES: usize = 1_000_000;
pub(crate) const MAX_WORDS: usize = 4096;
pub(crate) const MAX_PHONES: usize = 100_000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WordPositions {
    pub source: String,
    pub ranges: Vec<Option<Range<usize>>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PositionedWord {
    pub range: Range<usize>,
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
    positions: &'a WordPositions,
    pending: Option<&'a crate::app::document::ssml::Pending>,
    boundaries: Vec<usize>,
}

struct PendingWord {
    state: WordState,
    range: Option<Range<usize>>,
    anchor: usize,
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
        Ok(Self {
            source: text.to_owned(),
            ranges,
        })
    }

    pub(crate) fn validate(&self, words: &[WordState]) -> Result<(), String> {
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
        if positions.source != text || positions.ranges.len() != words.len() {
            return Err("Original word positions belong to different sentence text".into());
        }
        let mut budget = ValidationBudget::default();
        for word in words {
            validate_word(word, &mut budget)?;
        }
        Ok(Self {
            text,
            words,
            positions,
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
        let words = self
            .words
            .iter()
            .zip(&self.positions.ranges)
            .filter_map(|(word, range)| {
                let range = range.as_ref()?;
                (range.start >= selection.start && range.end <= selection.end).then(|| {
                    PositionedWord {
                        range: range.start - selection.start..range.end - selection.start,
                        state: word.clone(),
                    }
                })
            })
            .collect();
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
    inserted: &[Fragment],
) -> Result<crate::app::document::Block, String> {
    use crate::app::document::SentenceState;
    let mut budget = ValidationBudget::default();
    for fragment in inserted {
        fragment.validate(&mut budget)?;
    }
    let boundaries = text_boundaries(&block.text)?;
    let length = boundaries.len() - 1;
    if selection.start > selection.end || selection.end > length {
        return Err("Text selection is outside its block".into());
    }
    let mut staged = block.clone();
    staged.sync_sentences()?;
    let pending_before = staged
        .sentences
        .iter()
        .any(|sentence| sentence.ssml.is_some())
        .then(|| staged.clone());
    let ranges = staged.sentence_ranges();
    let first = ranges
        .iter()
        .position(|range| range.chars.end > selection.start)
        .unwrap_or(ranges.len().saturating_sub(1));
    let last = ranges
        .iter()
        .rposition(|range| range.chars.start < selection.end.max(selection.start + 1))
        .unwrap_or(first);
    let lower = ranges.get(first).map_or(0, |range| range.chars.start);
    let upper = ranges.get(last).map_or(0, |range| range.chars.end);
    let mut text = block.text[boundaries[lower]..boundaries[selection.start]].to_owned();
    let prefix = text.chars().count();
    let mut forced = Vec::new();
    let mut pasted = Vec::new();
    let mut count = prefix;
    for (index, fragment) in inserted.iter().enumerate() {
        if index != 0 {
            forced.push(count);
        }
        pasted.extend(fragment.words.iter().map(|word| PendingWord {
            state: word.state.clone(),
            range: Some(word.range.start + count..word.range.end + count),
            anchor: word.range.start + count,
        }));
        text.push_str(&fragment.text);
        count += fragment.text.chars().count();
    }
    let added = count - prefix;
    text.push_str(&block.text[boundaries[selection.end]..boundaries[upper]]);
    let inserted_bytes = inserted.iter().try_fold(0usize, |total, fragment| {
        total
            .checked_add(fragment.text.len())
            .ok_or("Clipboard text size overflow")
    })?;
    if block.text.len() - (boundaries[selection.end] - boundaries[selection.start]) + inserted_bytes
        > MAX_TEXT_BYTES
    {
        return Err("Edited block exceeds its text limit".into());
    }
    let mut before = Vec::new();
    let mut after = Vec::new();
    for (index, sentence) in staged
        .sentences
        .iter()
        .enumerate()
        .take(last + 1)
        .skip(first)
    {
        if !selection.is_empty()
            && selection.start <= ranges[index].chars.start
            && selection.end >= ranges[index].chars.end
        {
            continue;
        }
        let valid = sentence.word_positions.source == sentence.text
            && sentence.word_positions.ranges.len() == sentence.words.len();
        for (word_index, word) in sentence.words.iter().enumerate() {
            if boundary(word) {
                continue;
            }
            let original = valid
                .then(|| sentence.word_positions.ranges[word_index].clone())
                .flatten()
                .map(|range| {
                    range.start + ranges[index].chars.start..range.end + ranges[index].chars.start
                });
            if !selection.is_empty()
                && original.as_ref().is_some_and(|range| {
                    range.start >= selection.start && range.end <= selection.end
                })
            {
                continue;
            }
            let target = original
                .as_ref()
                .map(|range| range.end <= selection.start)
                .unwrap_or(false);
            let moved = original.as_ref().and_then(|range| {
                if range.end <= selection.start {
                    Some(range.start - lower..range.end - lower)
                } else if range.start >= selection.end {
                    let shift = selection.start - lower + added;
                    Some(range.start - selection.end + shift..range.end - selection.end + shift)
                } else {
                    None
                }
            });
            let owner_start = original
                .as_ref()
                .map_or(ranges[index].chars.start, |range| range.start);
            let anchor = moved.as_ref().map_or_else(
                || {
                    if owner_start < selection.start {
                        owner_start.saturating_sub(lower)
                    } else if owner_start >= selection.end {
                        prefix + added + owner_start - selection.end
                    } else {
                        prefix + added
                    }
                },
                |range| range.start,
            );
            let pending = PendingWord {
                state: word.clone(),
                range: moved,
                anchor,
            };
            if target {
                before.push(pending);
            } else {
                after.push(pending);
            }
        }
    }
    before.extend(pasted);
    before.extend(after);
    let byte_boundaries = text_boundaries(&text)?;
    let mut breaks = Vec::new();
    if inserted.len() < 2 {
        breaks.extend(
            crate::app::sentences::partition(&text)
                .iter()
                .map(|range| range.chars.end),
        );
    } else {
        forced.push(byte_boundaries.len() - 1);
        let mut group_start = 0;
        for (index, group_end) in forced.into_iter().enumerate() {
            let merges_prefix = index == 0 && prefix != 0;
            let merges_suffix = index + 1 == inserted.len() && upper > selection.end;
            if merges_prefix || merges_suffix {
                let group = &text[byte_boundaries[group_start]..byte_boundaries[group_end]];
                breaks.extend(
                    crate::app::sentences::partition(group)
                        .iter()
                        .map(|range| range.chars.end + group_start),
                );
            } else {
                breaks.push(group_end);
            }
            group_start = group_end;
        }
    }
    breaks.sort_unstable();
    breaks.dedup();
    if breaks.len() > 10_000 {
        return Err("Edited sentence count limit".into());
    }
    let mut updated = Vec::new();
    let mut output_budget = ValidationBudget::default();
    let mut start = 0;
    for end in breaks {
        if end <= start {
            continue;
        }
        let part = text[byte_boundaries[start]..byte_boundaries[end]].to_owned();
        let mut words = Vec::new();
        let mut positions = Vec::new();
        for pending in &before {
            if pending.anchor < start
                || (pending.anchor >= end
                    && !(end == byte_boundaries.len() - 1 && pending.anchor == end))
            {
                continue;
            }
            validate_word(&pending.state, &mut output_budget)?;
            words.push(pending.state.clone());
            positions.push(pending.range.as_ref().and_then(|range| {
                (range.start >= start && range.end <= end)
                    .then(|| range.start - start..range.end - start)
            }));
        }
        staged.sentence_serial = staged
            .sentence_serial
            .checked_add(1)
            .ok_or("Sentence identity exhausted")?;
        let native_owner = ranges
            .iter()
            .find(|r| r.chars.start == lower + start && r.chars.end == lower + end)
            .and_then(|r| staged.sentences.get(r.index))
            .filter(|s| s.text == part)
            .and_then(|s| s.native_owner);
        updated.push(SentenceState {
            native_owner,
            id: staged.sentence_serial,
            text: part.clone(),
            words,
            word_positions: WordPositions {
                source: part,
                ranges: positions,
            },
            query: Default::default(),
            ssml: None,
        });
        start = end;
    }
    if ranges.is_empty() {
        staged.sentences = updated;
    } else {
        staged.sentences.splice(first..=last, updated);
    }
    staged.text = staged
        .sentences
        .iter()
        .map(|sentence| sentence.text.as_str())
        .collect();
    staged.segments.clear();
    if let Some(before) = pending_before {
        crate::app::document::ssml::project(&before, &mut [&mut staged], Some(selection.clone()))?;
    }
    let mut cursor = boundaries[selection.start];
    let mut incoming = Vec::new();
    for fragment in inserted {
        if let Some(pending) = &fragment.ssml {
            incoming.push((cursor, pending));
        }
        cursor += fragment.text.len();
    }
    if !incoming.is_empty() {
        crate::app::document::ssml::overlay(&mut staged, &incoming)?;
    }
    staged.validate_sentences()?;
    Ok(staged)
}
