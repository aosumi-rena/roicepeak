//! Sentence ownership and model edits shared by text input and preparation.
use super::{Block, Project, SentenceState, WordState, alignment, state};
use crate::app::sentences::{self, SentenceRef};
impl Block {
    pub fn resolved_sentences(&self) -> Result<Vec<SentenceState>, String> {
        let mut copy = Block::new(self.id);
        copy.text = self.text.clone();
        copy.sentences = self.sentences.clone();
        copy.sentence_serial = self.sentence_serial;
        copy.sync_sentences()?;
        Ok(copy.sentences)
    }
    pub fn sync_sentences(&mut self) -> Result<(), String> {
        if !self.sentences.is_empty()
            && self
                .sentences
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
                == self.text
        {
            return Ok(());
        }
        let pending_before = self.sentences.iter().any(|s| s.ssml.is_some()).then(|| {
            let mut old = self.clone();
            old.text = old.sentences.iter().map(|s| s.text.as_str()).collect();
            old
        });
        let ranges = sentences::partition(&self.text);
        let texts: Vec<_> = ranges.iter().map(|r| &self.text[r.bytes.clone()]).collect();
        if self.sentences.len() == texts.len()
            && self.sentences.iter().zip(&texts).all(|(a, b)| a.text == *b)
        {
            return Ok(());
        }
        let old: Vec<_> = self.sentences.iter().map(|s| s.text.as_str()).collect();
        let changes = alignment::align(&old, &texts)?;
        let mut kept = vec![None; texts.len()];
        for change in changes {
            if change.kind <= 1 {
                if let (Some(a), Some(b)) = (change.old, change.new) {
                    kept[b] = Some(self.sentences[a].clone());
                }
            }
        }
        let mut updated = Vec::with_capacity(texts.len());
        let mut serial = self.sentence_serial;
        for (i, text) in texts.into_iter().enumerate() {
            let mut sentence = if let Some(s) = kept[i].take() {
                s
            } else {
                serial = serial.checked_add(1).ok_or("Sentence identity exhausted")?;
                SentenceState {
                    native_owner: None,
                    id: serial,
                    text: String::new(),
                    words: Vec::new(),
                    word_positions: Default::default(),
                    query: Default::default(),
                    ssml: None,
                }
            };
            sentence.text = text.to_owned();
            updated.push(sentence);
        }
        self.sentences = updated;
        self.sentence_serial = serial;
        if let Some(before) = pending_before {
            super::ssml::project(&before, &mut [&mut *self], None)?;
        }
        self.segments.clear();
        Ok(())
    }
    pub fn set_text(&mut self, text: String) -> Result<(), String> {
        let mut staged = self.clone();
        staged.text = text;
        staged.sync_sentences()?;
        *self = staged;
        Ok(())
    }
    pub fn sentence_ranges(&self) -> Vec<SentenceRef> {
        if self.sentences.is_empty()
            || self
                .sentences
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
                != self.text
        {
            return sentences::partition(&self.text);
        }
        let (mut byte, mut scalar) = (0, 0);
        self.sentences
            .iter()
            .enumerate()
            .map(|(index, s)| {
                let start_byte = byte;
                let start_scalar = scalar;
                byte += s.text.len();
                scalar += s.text.chars().count();
                SentenceRef {
                    index,
                    bytes: start_byte..byte,
                    chars: start_scalar..scalar,
                }
            })
            .collect()
    }
    pub fn sentence_state(&self, address: &SentenceRef) -> Option<&SentenceState> {
        self.sentences
            .get(address.index)
            .filter(|s| self.text.get(address.bytes.clone()) == Some(s.text.as_str()))
    }
    pub fn replace_words(
        &mut self,
        address: &SentenceRef,
        words: Vec<WordState>,
    ) -> Result<(), String> {
        self.sync_sentences()?;
        let state = self
            .sentences
            .get_mut(address.index)
            .ok_or("Sentence is no longer present")?;
        state.words = words;
        state.word_positions = Default::default();
        Ok(())
    }
    pub fn split_at(&mut self, new_id: u64, caret: usize) -> Result<Block, String> {
        let mut staged = self.clone();
        staged.sync_sentences()?;
        let pending_before = staged
            .sentences
            .iter()
            .any(|sentence| sentence.ssml.is_some())
            .then(|| staged.clone());
        let mut right = staged.split_inner(new_id, caret)?;
        if let Some(before) = pending_before {
            super::ssml::project(&before, &mut [&mut staged, &mut right], None)?;
        }
        *self = staged;
        Ok(right)
    }
    fn split_inner(&mut self, new_id: u64, caret: usize) -> Result<Block, String> {
        let mut right = Block::new(new_id);
        right.narrator = self.narrator.clone();
        right.parameters = self.parameters.clone();
        right.offset_mode = self.offset_mode;
        right.offset_seconds = self.offset_seconds;
        right.sentence_serial = self.sentence_serial;
        if caret >= self.text.chars().count() {
            return Ok(right);
        }
        let ranges = self.sentence_ranges();
        let index = ranges
            .iter()
            .position(|r| r.chars.contains(&caret))
            .ok_or("Caret is outside stored sentence ranges")?;
        right.sentences = self.sentences.split_off(index + 1);
        if caret == ranges[index].chars.start {
            let moved = self.sentences.pop().ok_or("Sentence disappeared")?;
            right.sentences.insert(0, moved);
        } else {
            let left = self.sentences.last_mut().ok_or("Sentence disappeared")?;
            let relative = caret - ranges[index].chars.start;
            let byte = left
                .text
                .char_indices()
                .nth(relative)
                .map_or(left.text.len(), |(at, _)| at);
            let tail = left.text.split_off(byte);
            right.sentence_serial = right
                .sentence_serial
                .checked_add(1)
                .ok_or("Sentence identity exhausted")?;
            right.sentences.insert(
                0,
                SentenceState {
                    native_owner: None,
                    id: right.sentence_serial,
                    text: tail,
                    words: left.words.clone(),
                    word_positions: left.word_positions.clone(),
                    query: left.query.clone(),
                    ssml: None,
                },
            );
        }
        self.text = self.sentences.iter().map(|s| s.text.as_str()).collect();
        right.text = right.sentences.iter().map(|s| s.text.as_str()).collect();
        self.segments.clear();
        right.segments.clear();
        Ok(right)
    }
    pub fn capture_lane_changes(&mut self, before: &Block) {
        if self.text != before.text {
            return;
        }
        for (index, segment) in self.segments.iter().enumerate() {
            let Some(old) = before.segments.get(index) else {
                continue;
            };
            if old.sentence != segment.sentence
                || old.token != segment.token
                || old.mora != segment.mora
                || old.phone != segment.phone
            {
                continue;
            }
            let Some(mora) = self
                .sentences
                .get_mut(segment.sentence.index)
                .and_then(|s| s.words.get_mut(segment.token))
                .and_then(|w| w.morae.get_mut(segment.mora))
            else {
                continue;
            };
            if old.high != segment.high || old.accent_edited != segment.accent_edited {
                mora.code = if !segment.accent_edited {
                    segment.accent_code
                } else if segment.high {
                    roicepeak::phonemes::HIGH
                } else {
                    roicepeak::phonemes::LOW
                };
                mora.accent_edited = segment.accent_edited;
            }
            if old.intonation_delta != segment.intonation_delta {
                mora.intonation_delta = segment.intonation_delta;
            }
            if old.duration_edit != segment.duration_edit {
                if let Some(phone) = mora.phones.get_mut(segment.phone) {
                    phone.duration = segment.duration_edit;
                }
            }
        }
        for segment in &mut self.segments {
            if let Some(sentence) = self.sentences.get(segment.sentence.index) {
                state::project(&sentence.words, std::slice::from_mut(segment));
            }
        }
    }
    pub fn validate_sentences(&self) -> Result<(), String> {
        if self.sentences.len() > 10_000 {
            return Err("Sentence count limit".into());
        }
        if !self.sentences.is_empty()
            && self
                .sentences
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>()
                != self.text
        {
            return Err("Canonical sentence text differs from block text".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut phones = [0usize; 5];
        for sentence in &self.sentences {
            sentence.word_positions.validate(&sentence.words)?;
            if sentence.text.is_empty()
                || sentence.id == 0
                || sentence.id > self.sentence_serial
                || !ids.insert(sentence.id)
                || sentence.words.len() > 4096
                || sentence.query.text.len() > 1_000_000
                || sentence.query.text.contains('\0')
                || sentence
                    .query
                    .snapshots()
                    .iter()
                    .any(|words| words.len() > 4096)
            {
                return Err("Invalid canonical sentence".into());
            }
            if let Some(pending) = &sentence.ssml {
                pending.validate(&sentence.text)?;
            }
            let snapshots = sentence.query.snapshots();
            if (sentence.query.text.is_empty() && snapshots.iter().any(|words| !words.is_empty()))
                || snapshots
                    .iter()
                    .any(|words| words.len() != sentence.query.raw.len())
            {
                return Err("Invalid derived word query shape".into());
            }
            for (lane, snapshot) in std::iter::once(&sentence.words)
                .chain(snapshots)
                .enumerate()
            {
                for word in snapshot {
                    if word.surface.len() > 1_000_000
                        || word.surface.contains('\0')
                        || !(1..=3).contains(&word.language)
                        || word.morae.len() > 100_000
                    {
                        return Err("Canonical word limit".into());
                    }
                    for mora in &word.morae {
                        if mora.reading.len() > 4096
                            || mora.reading.contains('\0')
                            || !mora.intonation_delta.is_finite()
                            || !roicepeak::prosody::CODES.contains(&mora.code)
                        {
                            return Err("Invalid canonical mora".into());
                        }
                        phones[lane] = phones[lane]
                            .checked_add(mora.phones.len())
                            .filter(|n| *n <= 100_000)
                            .ok_or("Word snapshot phone limit")?;
                        if mora
                            .phones
                            .iter()
                            .any(|p| p.symbol.len() > 4096 || !p.duration.valid())
                        {
                            return Err("Invalid canonical phone".into());
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
impl Project {
    pub fn synchronize_sentences(&mut self) -> Result<(), String> {
        for block in &mut self.blocks {
            block.sync_sentences()?;
        }
        Ok(())
    }
    pub fn commit_edit_state(&mut self, before: &Project) -> Result<(), String> {
        for block in &mut self.blocks {
            if let Some(old) = before.blocks.iter().find(|old| old.id == block.id) {
                block.capture_lane_changes(old);
            }
            block.sync_sentences()?;
        }
        Ok(())
    }
}
