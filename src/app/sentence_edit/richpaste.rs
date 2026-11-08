use super::{Fragment, MAX_TEXT_BYTES, ValidationBudget, WordPositions, boundary, text_boundaries};
use crate::app::document::{Block, SentenceState, ssml};
use std::ops::Range;

pub(crate) fn replace_rich(
    block: &Block,
    selection: Range<usize>,
    inserted: &[Fragment],
) -> Result<Block, String> {
    if inserted.len() > 10_000 {
        return Err("Edited sentence count limit".into());
    }
    let mut budget = ValidationBudget::default();
    let mut added = 0usize;
    for fragment in inserted {
        fragment.validate(&mut budget)?;
        added = added
            .checked_add(fragment.text.len())
            .ok_or("Clipboard text size overflow")?;
    }
    let at = text_boundaries(&block.text)?;
    if selection.start > selection.end || selection.end >= at.len() {
        return Err("Text selection is outside its block".into());
    }
    if (block.text.len() - (at[selection.end] - at[selection.start]))
        .checked_add(added)
        .is_none_or(|length| length > MAX_TEXT_BYTES)
    {
        return Err("Edited block exceeds its text limit".into());
    }
    let mut staged = block.clone();
    staged.sync_sentences()?;
    if !selection.is_empty() {
        let before = staged.clone();
        staged
            .text
            .replace_range(at[selection.start]..at[selection.end], "");
        staged.sync_partition()?;
        ssml::project(&before, &mut [&mut staged], Some(selection.clone()))?;
    } else if staged.text.is_empty() {
        staged.sync_partition()?;
    }
    for sentence in &mut staged.sentences {
        sentence.word_positions.raw_ranges = sentence
            .word_positions
            .stored_ranges(sentence.words.len())?;
    }
    let mut editor = Editor { block: staged };
    if !inserted.is_empty() {
        let length = editor.block.text.chars().count();
        let ranges = editor.block.sentence_ranges();
        let destination = if selection.start == length {
            ranges.last()
        } else {
            ranges
                .iter()
                .find(|range| range.chars.contains(&selection.start))
        };
        if let Some(destination) = destination {
            editor.insert(
                destination.index,
                selection.start - destination.chars.start,
                inserted,
            )?;
        } else if editor.block.sentences.is_empty() {
            for fragment in inserted {
                editor.whole(editor.block.sentences.len(), fragment)?;
            }
        } else {
            return Err("Paste destination is outside stored sentence ranges".into());
        }
    }
    editor.block.text = editor
        .block
        .sentences
        .iter()
        .map(|sentence| sentence.text.as_str())
        .collect();
    editor.block.segments.clear();
    editor.block.validate_sentences()?;
    Ok(editor.block)
}

struct Editor {
    block: Block,
}

impl Editor {
    fn identity(&mut self) -> Result<u64, String> {
        self.block.sentence_serial = self
            .block
            .sentence_serial
            .checked_add(1)
            .ok_or("Sentence identity exhausted")?;
        Ok(self.block.sentence_serial)
    }

    fn append(&mut self, index: usize, sentence: SentenceState) -> Result<(), String> {
        if self.block.sentences.len() >= 10_000 || index > self.block.sentences.len() {
            return Err("Edited sentence count or position limit".into());
        }
        self.block.sentences.insert(index, sentence);
        Ok(())
    }

    fn whole(&mut self, index: usize, fragment: &Fragment) -> Result<(), String> {
        let sentence = SentenceState {
            id: self.identity()?,
            native_owner: None,
            text: fragment.text.clone(),
            has_eos: fragment.completes_sentence,
            words: fragment
                .words
                .iter()
                .map(|word| word.state.clone())
                .collect(),
            word_positions: WordPositions {
                source: fragment.text.clone(),
                ranges: fragment
                    .words
                    .iter()
                    .map(|word| Some(word.range.clone()))
                    .collect(),
                raw_ranges: fragment.words.iter().map(|word| word.raw_range).collect(),
            },
            query: Default::default(),
            ssml: fragment.ssml.clone(),
        };
        self.append(index, sentence)
    }

    fn split(&mut self, index: usize, caret: usize) -> Result<Option<SentenceState>, String> {
        let before = self
            .block
            .sentences
            .get(index)
            .ok_or("Paste sentence is no longer present")?
            .clone();
        let at = text_boundaries(&before.text)?;
        let length = at.len() - 1;
        if caret == 0 || caret >= length {
            return Ok(None);
        }
        let mapped = mapped_ranges(&before);
        let raw = before.word_positions.stored_ranges(before.words.len())?;
        let mut right = before.clone();
        right.id = self.identity()?;
        right.native_owner = None;
        right.text = before.text[at[caret]..].to_owned();
        right.has_eos = true;
        right.word_positions = WordPositions {
            source: right.text.clone(),
            ranges: mapped
                .iter()
                .map(|range| {
                    range.as_ref().and_then(|range| {
                        (range.start >= caret).then(|| range.start - caret..range.end - caret)
                    })
                })
                .collect(),
            raw_ranges: raw.clone(),
        };
        right.query = Default::default();
        right.ssml = before
            .ssml
            .as_ref()
            .map(|pending| ssml::slice(pending, caret..length))
            .transpose()?
            .flatten();
        let left = &mut self.block.sentences[index];
        left.text = before.text[..at[caret]].to_owned();
        left.word_positions = WordPositions {
            source: left.text.clone(),
            ranges: mapped
                .into_iter()
                .map(|range| range.filter(|range| range.end <= caret))
                .collect(),
            raw_ranges: raw,
        };
        left.query = Default::default();
        left.ssml = before
            .ssml
            .as_ref()
            .map(|pending| ssml::slice(pending, 0..caret))
            .transpose()?
            .flatten();
        Ok(Some(right))
    }

    fn merge(&mut self, index: usize, fragment: &Fragment, offset: i32) -> Result<(), String> {
        let sentence = self
            .block
            .sentences
            .get_mut(index)
            .ok_or("Paste sentence is no longer present")?;
        let before = sentence.clone();
        let raw = before.word_positions.stored_ranges(before.words.len())?;
        if before
            .words
            .iter()
            .zip(&raw)
            .any(|(word, range)| !boundary(word) && range.is_none())
        {
            return Err("Stored word positions are unavailable for structured paste".into());
        }
        let insertion = raw
            .iter()
            .position(|range| range.is_some_and(|[start, end]| start <= offset && offset < end))
            .unwrap_or(before.words.len());
        let at = text_boundaries(&before.text)?;
        let caret = usize::try_from(offset).unwrap_or(0).min(at.len() - 1);
        let added = fragment.text.chars().count();
        let mut mapped = mapped_ranges(&before);
        for range in &mut mapped {
            *range = range.as_ref().and_then(|range| {
                if range.end <= caret {
                    Some(range.clone())
                } else if range.start >= caret {
                    Some(range.start + added..range.end + added)
                } else {
                    None
                }
            });
        }
        mapped.splice(
            insertion..insertion,
            fragment
                .words
                .iter()
                .map(|word| Some(word.range.start + caret..word.range.end + caret)),
        );
        let mut raw_ranges = raw;
        raw_ranges.splice(
            insertion..insertion,
            fragment.words.iter().map(|word| word.raw_range),
        );
        sentence.words.splice(
            insertion..insertion,
            fragment.words.iter().map(|word| word.state.clone()),
        );
        sentence.text.insert_str(at[caret], &fragment.text);
        sentence.word_positions = WordPositions {
            source: sentence.text.clone(),
            ranges: mapped,
            raw_ranges,
        };
        sentence.query = Default::default();
        sentence.ssml = None;
        project_annotations(&before, sentence, caret, fragment)?;
        Ok(())
    }

    fn single(&mut self, index: usize, caret: usize, fragment: &Fragment) -> Result<usize, String> {
        let destination = self
            .block
            .sentences
            .get(index)
            .ok_or("Paste sentence is no longer present")?;
        let at = text_boundaries(&destination.text)?;
        let length = at.len() - 1;
        if caret > length {
            return Err("Paste caret is outside its sentence".into());
        }
        let mut combined = destination.text.clone();
        combined.insert_str(at[caret], &fragment.text);
        let partitions = crate::app::sentences::partition(&combined);
        if partitions.len() < 2 {
            self.merge(index, fragment, caret as i32)?;
            return Ok(index);
        }
        if caret == 0 {
            self.whole(index, fragment)?;
            return Ok(index + 1);
        }
        if caret == length {
            self.whole(index + 1, fragment)?;
            return Ok(index + 1);
        }
        let first = &partitions[0].chars;
        let aligned = first.end == caret + fragment.text.chars().count();
        let offset = caret as i32 + first.start as i32 - first.end as i32;
        let mut right = self
            .split(index, caret)?
            .ok_or("Paste split did not produce a right sentence")?;
        if aligned {
            self.merge(index, fragment, caret as i32)?;
            self.append(index + 1, right)?;
        } else {
            std::mem::swap(&mut self.block.sentences[index], &mut right);
            self.merge(index, fragment, offset)?;
            std::mem::swap(&mut self.block.sentences[index], &mut right);
            self.append(index + 1, right)?;
        }
        Ok(index + 1)
    }

    fn insert(&mut self, index: usize, caret: usize, inserted: &[Fragment]) -> Result<(), String> {
        if inserted.len() == 1 {
            self.single(index, caret, &inserted[0])?;
        } else if caret == 0 {
            for (offset, fragment) in inserted[..inserted.len() - 1].iter().enumerate() {
                self.whole(index + offset, fragment)?;
            }
            self.single(index + inserted.len() - 1, 0, &inserted[inserted.len() - 1])?;
        } else {
            let right = self.split(index, caret)?;
            let first = self.single(index, caret, &inserted[0])?;
            for (offset, fragment) in inserted[1..inserted.len() - 1].iter().enumerate() {
                self.whole(first + offset + 1, fragment)?;
            }
            let last = first + inserted.len() - 1;
            if let Some(right) = right {
                self.append(last, right)?;
                self.single(last, 0, &inserted[inserted.len() - 1])?;
            } else {
                self.whole(last, &inserted[inserted.len() - 1])?;
            }
        }
        Ok(())
    }
}

fn mapped_ranges(sentence: &SentenceState) -> Vec<Option<Range<usize>>> {
    if sentence.word_positions.source == sentence.text
        && sentence.word_positions.ranges.len() == sentence.words.len()
    {
        sentence.word_positions.ranges.clone()
    } else {
        vec![None; sentence.words.len()]
    }
}

fn project_annotations(
    before: &SentenceState,
    after: &mut SentenceState,
    caret: usize,
    fragment: &Fragment,
) -> Result<(), String> {
    if before.ssml.is_none() && fragment.ssml.is_none() {
        return Ok(());
    }
    let mut original = Block::new(1);
    original.text = before.text.clone();
    original.sentences = vec![before.clone()];
    let mut target = Block::new(1);
    target.text = after.text.clone();
    target.sentences = vec![after.clone()];
    ssml::project(&original, &mut [&mut target], Some(caret..caret))?;
    if let Some(pending) = &fragment.ssml {
        let at = text_boundaries(&before.text)?;
        ssml::overlay(&mut target, &[(at[caret], pending)])?;
    }
    after.ssml = target.sentences.remove(0).ssml;
    Ok(())
}
