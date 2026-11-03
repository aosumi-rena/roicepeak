use crate::app::document::{Block, state};
use std::{collections::HashMap, ops::Range};

pub(super) struct Word {
    pub(super) sentence_index: usize,
    pub(super) language: u32,
    pub(super) sentence_id: u64,
    pub(super) token: usize,
    pub(super) text: String,
    pub(super) morae: Vec<Mora>,
}
pub(super) struct Mora {
    pub(super) index: usize,
    pub(super) reading: String,
    pub(super) high: bool,
    pub(super) intonation: f32,
    pub(super) raw_intonation: f32,
    pub(super) duration: Option<state::DurationState>,
    pub(super) raw_query_duration_seconds: Option<f64>,
    sentence: usize,
    sentence_id: u64,
    token: usize,
    phones: Range<usize>,
}
struct Projection {
    phones: Range<usize>,
    high: bool,
    intonation: f32,
    raw_intonation: f32,
    raw_duration: Option<(usize, f64)>,
}

pub(super) fn words(block: &Block) -> Vec<Word> {
    let mut projections = HashMap::new();
    for (index, segment) in block.segments.iter().enumerate() {
        let key = (segment.sentence.index, segment.token, segment.mora);
        let projection = projections.entry(key).or_insert_with(|| Projection {
            phones: index..index + 1,
            high: segment.high,
            intonation: segment.intonation,
            raw_intonation: segment.raw_query_intonation,
            raw_duration: None,
        });
        projection.phones.end = index + 1;
        let phone = block
            .sentences
            .get(segment.sentence.index)
            .and_then(|sentence| sentence.words.get(segment.token))
            .and_then(|word| {
                word.morae
                    .get(segment.mora)
                    .and_then(|mora| mora.phones.get(segment.phone))
                    .map(|phone| (word.language, phone))
            });
        if let Some((language, phone)) = phone {
            if state::duration_phone(language, &phone.symbol, phone.nucleus)
                && segment.raw_query_duration_seconds.is_finite()
                && segment.raw_query_duration_seconds >= 0.0
                && projection
                    .raw_duration
                    .is_none_or(|(phone, _)| segment.phone < phone)
            {
                projection.raw_duration = Some((segment.phone, segment.raw_query_duration_seconds));
            }
        }
    }
    let mut words = Vec::new();
    for (sentence_index, sentence) in block.sentences.iter().enumerate() {
        for (token, word) in sentence.words.iter().enumerate() {
            if matches!(word.pos, 0x2000 | 0x2001) {
                continue;
            }
            let morae = word
                .morae
                .iter()
                .enumerate()
                .map(|(index, mora)| {
                    let projection = projections.get(&(sentence_index, token, index));
                    let raw_intonation = projection.map_or(0.0, |p| p.raw_intonation);
                    let duration = mora
                        .duration_mode(word.language)
                        .zip(mora.duration_value(word.language))
                        .map(|(mode, value)| state::DurationState { mode, value });
                    let first = mora.phones.iter().position(|phone| {
                        state::duration_phone(word.language, &phone.symbol, phone.nucleus)
                    });
                    let raw_query_duration_seconds = projection
                        .and_then(|p| p.raw_duration)
                        .filter(|(phone, _)| Some(*phone) == first)
                        .map(|(_, value)| value);
                    Mora {
                        index,
                        reading: mora.reading.clone(),
                        high: projection.map_or(mora.code == roicepeak::phonemes::HIGH, |p| p.high),
                        intonation: projection
                            .map_or(mora.intonation_delta + raw_intonation, |p| p.intonation),
                        raw_intonation,
                        duration,
                        raw_query_duration_seconds,
                        sentence: sentence_index,
                        sentence_id: sentence.id,
                        token,
                        phones: projection.map_or(0..0, |p| p.phones.clone()),
                    }
                })
                .collect();
            words.push(Word {
                sentence_index,
                language: word.language,
                sentence_id: sentence.id,
                token,
                text: word.surface.clone(),
                morae,
            });
        }
    }
    words
}
impl Mora {
    fn state<'a>(&self, block: &'a mut Block) -> Option<&'a mut state::MoraState> {
        block
            .sentences
            .get_mut(self.sentence)
            .filter(|sentence| sentence.id == self.sentence_id)
            .and_then(|sentence| sentence.words.get_mut(self.token))
            .and_then(|word| word.morae.get_mut(self.index))
    }
    pub(super) fn accent(&self, block: &mut Block, high: bool) {
        if let Some(mora) = self.state(block) {
            let code = if high {
                roicepeak::phonemes::HIGH
            } else {
                roicepeak::phonemes::LOW
            };
            if mora.code == code {
                return;
            }
            mora.code = code;
            mora.accent_edited = true;
        }
        self.project(block);
    }
    pub(super) fn intonation(&self, block: &mut Block, target: f32) {
        let delta = target - self.raw_intonation;
        if !delta.is_finite() {
            return;
        }
        if let Some(mora) = self.state(block) {
            mora.intonation_delta = delta;
        }
        self.project(block);
    }
    pub(super) fn duration(&self, block: &mut Block, value: state::DurationState) {
        if !value.valid() {
            return;
        }
        self.update_duration(block, |mora, language| mora.set_duration(language, value));
    }
    pub(super) fn duration_value(&self, block: &mut Block, value: f64) {
        if !value.is_finite() || value < 0.0 {
            return;
        }
        self.update_duration(block, |mora, language| {
            mora.set_duration_value(language, value)
        });
    }
    fn update_duration(&self, block: &mut Block, apply: impl FnOnce(&mut state::MoraState, u32)) {
        if let Some(word) = block
            .sentences
            .get_mut(self.sentence)
            .filter(|sentence| sentence.id == self.sentence_id)
            .and_then(|sentence| sentence.words.get_mut(self.token))
        {
            let language = word.language;
            if let Some(mora) = word.morae.get_mut(self.index) {
                apply(mora, language);
            }
        }
        self.project(block);
    }
    fn project(&self, block: &mut Block) {
        let Some(sentence) = block
            .sentences
            .get(self.sentence)
            .filter(|sentence| sentence.id == self.sentence_id)
        else {
            return;
        };
        let Some(segments) = block.segments.get_mut(self.phones.clone()) else {
            return;
        };
        state::project(&sentence.words, segments);
    }
}
