//! Authoring state is independent of derived timeline phone cells.
use super::{Segment, alignment};
use roicepeak::{
    japanese::Analysis,
    phonemes::{Phone, Syllable, Token},
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DurationMode {
    Relative,
    Absolute,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct DurationState {
    pub mode: DurationMode,
    pub value: f64,
}
impl Default for DurationState {
    fn default() -> Self {
        Self {
            mode: DurationMode::Relative,
            value: 1.0,
        }
    }
}
impl DurationState {
    pub fn valid(self) -> bool {
        self.value.is_finite() && self.value >= 0.0
    }
    pub fn edited(self) -> bool {
        self != Self::default()
    }
    pub fn converted(self, mode: DurationMode, raw_seconds: f64) -> Option<Self> {
        if !raw_seconds.is_finite() || raw_seconds < 0.0 || !self.valid() {
            return None;
        }
        if self.mode == mode {
            return Some(self);
        }
        let value = match mode {
            DurationMode::Absolute => (self.value * raw_seconds).clamp(0.01, 10.0),
            DurationMode::Relative if raw_seconds > 0.0 => {
                (self.value / raw_seconds).clamp(0.5, 2.0)
            }
            DurationMode::Relative => 0.0,
        };
        Some(Self { mode, value })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PhoneState {
    pub symbol: String,
    pub nucleus: bool,
    pub duration: DurationState,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MoraState {
    pub reading: String,
    pub code: u32,
    pub prediction_eligible: bool,
    pub accent_edited: bool,
    pub intonation_delta: f32,
    pub phones: Vec<PhoneState>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WordState {
    pub surface: String,
    pub language: u32,
    pub pos: u32,
    pub pronunciation_edited: bool,
    pub morae: Vec<MoraState>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SentenceState {
    pub id: u64,
    #[serde(deserialize_with = "crate::app::native_origin::required_owner")]
    pub(crate) native_owner: Option<crate::app::native_origin::NativeOwner>,
    pub text: String,
    pub words: Vec<WordState>,
    pub word_positions: crate::app::sentence_edit::WordPositions,
    pub query: WordQueryState,
    #[serde(deserialize_with = "super::ssml::required")]
    pub ssml: Option<super::ssml::Pending>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct WordQueryState {
    pub text: String,
    pub raw: Vec<WordState>,
    pub pronunciation: Vec<WordState>,
    pub predicted: Vec<WordState>,
    pub controlled: Vec<WordState>,
}
impl WordQueryState {
    pub fn matches(&self, text: &str, words: &[WordState]) -> bool {
        self.text == text
            && self.controlled.len() == words.len()
            && words
                .iter()
                .zip(&self.controlled)
                .all(|(a, b)| a.matches_query(b))
    }
    pub fn snapshots(&self) -> [&Vec<WordState>; 4] {
        [
            &self.raw,
            &self.pronunciation,
            &self.predicted,
            &self.controlled,
        ]
    }
}
impl WordState {
    pub fn matches_query(&self, query: &Self) -> bool {
        self.surface == query.surface
            && self.language == query.language
            && self.morae.len() == query.morae.len()
            && self
                .morae
                .iter()
                .zip(&query.morae)
                .all(|(a, b)| a.reading == b.reading)
    }

    pub fn reading(&self) -> String {
        self.morae.iter().map(|m| m.reading.as_str()).collect()
    }
    pub fn has_duration_edits(&self) -> bool {
        self.morae
            .iter()
            .any(|m| m.phones.iter().any(|p| p.duration.edited()))
    }
    pub fn reset_duration(&mut self) {
        for mora in &mut self.morae {
            mora.reset_duration();
        }
    }
    pub fn reset_intonation(&mut self) {
        for mora in &mut self.morae {
            mora.intonation_delta = 0.0;
        }
    }
    pub fn reset_reading(&mut self, baseline: &Self) -> bool {
        if !self.pronunciation_edited && self.reading() == baseline.reading() {
            return false;
        }
        *self = baseline.clone();
        self.pronunciation_edited = false;
        true
    }
    pub fn replace_reading(&mut self, baseline: &Self, token: &Token) -> bool {
        let reading: String = token.syllables.iter().map(|m| m.reading.as_str()).collect();
        if reading == self.reading() {
            return false;
        }
        if reading == baseline.reading() {
            return self.reset_reading(baseline);
        }
        self.morae = Self::from_token(token, &vec![true; token.syllables.len()]).morae;
        for (index, mora) in self.morae.iter_mut().enumerate() {
            mora.code = if self.language == 1 {
                if index == 0 {
                    roicepeak::phonemes::LOW
                } else {
                    roicepeak::phonemes::HIGH
                }
            } else {
                roicepeak::phonemes::NONE
            };
        }
        self.pronunciation_edited = true;
        true
    }
    fn from_token(token: &Token, predict: &[bool]) -> Self {
        Self {
            surface: token.text.clone(),
            language: token.language,
            pos: token.pos,
            pronunciation_edited: false,
            morae: token
                .syllables
                .iter()
                .zip(predict)
                .map(|(s, eligible)| MoraState {
                    reading: s.reading.clone(),
                    code: s.code,
                    prediction_eligible: *eligible,
                    accent_edited: false,
                    intonation_delta: 0.0,
                    phones: s
                        .phones
                        .iter()
                        .map(|p| PhoneState {
                            symbol: p.symbol.clone(),
                            nucleus: p.nucleus,
                            duration: DurationState::default(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }
    fn token(&self) -> Token {
        Token {
            text: self.surface.clone(),
            language: self.language,
            pos: self.pos,
            syllables: self
                .morae
                .iter()
                .map(|m| Syllable {
                    reading: m.reading.clone(),
                    code: m.code,
                    phones: m
                        .phones
                        .iter()
                        .map(|p| Phone {
                            symbol: p.symbol.clone(),
                            nucleus: p.nucleus,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
    pub fn manual(&self) -> bool {
        self.pronunciation_edited
            || self.morae.iter().any(|m| {
                m.accent_edited
                    || m.intonation_delta != 0.0
                    || m.phones.iter().any(|p| p.duration.edited())
            })
    }
}
impl MoraState {
    pub fn duration_mode(&self, language: u32) -> Option<DurationMode> {
        self.phones
            .iter()
            .find(|p| duration_phone(language, &p.symbol, p.nucleus))
            .map(|p| p.duration.mode)
    }
    pub fn duration_value(&self, language: u32) -> Option<f64> {
        self.phones
            .iter()
            .rev()
            .find(|p| duration_phone(language, &p.symbol, p.nucleus))
            .map(|p| p.duration.value)
    }
    pub fn first_duration(&self, language: u32) -> Option<DurationState> {
        self.phones
            .iter()
            .find(|p| duration_phone(language, &p.symbol, p.nucleus))
            .map(|p| p.duration)
    }
    pub fn set_duration(&mut self, language: u32, value: DurationState) {
        for phone in &mut self.phones {
            if duration_phone(language, &phone.symbol, phone.nucleus) {
                phone.duration = value;
            }
        }
    }
    pub fn set_duration_value(&mut self, language: u32, value: f64) {
        for phone in &mut self.phones {
            if duration_phone(language, &phone.symbol, phone.nucleus) {
                phone.duration.value = value;
            }
        }
    }
    pub fn reset_duration(&mut self) {
        for phone in &mut self.phones {
            phone.duration = DurationState::default();
        }
    }
}
pub fn duration_phone(language: u32, symbol: &str, nucleus: bool) -> bool {
    matches!(symbol, "cl" | "sil" | "pau")
        || if language == 1 {
            matches!(symbol, "a" | "i" | "u" | "e" | "o" | "N")
        } else {
            nucleus
        }
}
fn boundaries(pos: u32) -> bool {
    matches!(pos, 0x2000 | 0x2001)
}
fn ordinary(words: &[WordState]) -> Vec<usize> {
    words
        .iter()
        .enumerate()
        .filter_map(|(i, w)| (!boundaries(w.pos)).then_some(i))
        .collect()
}
pub fn from_analysis(analysis: &Analysis) -> Result<Vec<WordState>, String> {
    if analysis.tokens.len() != analysis.predict.len()
        || analysis
            .tokens
            .iter()
            .zip(&analysis.predict)
            .any(|(token, predict)| token.syllables.len() != predict.len())
    {
        return Err("Analysis prediction eligibility dimensions differ from its moras".into());
    }
    Ok(analysis
        .tokens
        .iter()
        .zip(&analysis.predict)
        .map(|(token, predict)| WordState::from_token(token, predict))
        .collect())
}
pub fn pronunciation_query(
    raw: &Analysis,
    saved: &[WordState],
) -> Result<(Analysis, Vec<WordState>), String> {
    let fresh = from_analysis(raw)?;
    let mut pronunciation = fresh.clone();
    let oi = ordinary(saved);
    let ni = ordinary(&fresh);
    let changes = alignment::align(
        &oi.iter()
            .map(|i| saved[*i].surface.as_str())
            .collect::<Vec<_>>(),
        &ni.iter()
            .map(|i| fresh[*i].surface.as_str())
            .collect::<Vec<_>>(),
    )?;
    let mut tokens: Vec<_> = fresh.iter().map(WordState::token).collect();
    let mut predict = raw.predict.clone();
    for change in changes {
        if change.kind != 0 {
            continue;
        }
        let (Some(a), Some(b)) = (change.old, change.new) else {
            continue;
        };
        let old = &saved[oi[a]];
        let index = ni[b];
        let new = &fresh[index];
        if old.pronunciation_edited {
            tokens[index] = Token {
                text: new.surface.clone(),
                language: new.language,
                pos: new.pos,
                syllables: old.token().syllables,
            };
            pronunciation[index].morae = old.morae.clone();
            pronunciation[index].pronunciation_edited = true;
            predict[index] = old
                .morae
                .iter()
                .map(|m| m.prediction_eligible && !m.accent_edited)
                .collect();
        }
    }
    Ok((
        Analysis {
            tokens,
            input_ranges: raw.input_ranges.clone(),
            input_text: raw.input_text.clone(),
            normalization_exclusions: raw.normalization_exclusions.clone(),
            predict,
        },
        pronunciation,
    ))
}
pub fn token_signature(analysis: &Analysis) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&from_analysis(analysis)?).map_err(|e| e.to_string())
}
pub fn reconcile(saved: &[WordState], fresh: &[WordState]) -> Result<Vec<WordState>, String> {
    let oi = ordinary(saved);
    let ni = ordinary(fresh);
    let changes = alignment::align(
        &oi.iter()
            .map(|i| saved[*i].surface.as_str())
            .collect::<Vec<_>>(),
        &ni.iter()
            .map(|i| fresh[*i].surface.as_str())
            .collect::<Vec<_>>(),
    )?;
    let mut output = fresh.to_vec();
    for change in changes {
        if change.kind > 1 {
            continue;
        }
        let (Some(a), Some(b)) = (change.old, change.new) else {
            continue;
        };
        retain(&saved[oi[a]], &mut output[ni[b]]);
    }
    for out in &mut output {
        if boundaries(out.pos) {
            if let Some(old) = saved.iter().find(|old| old.pos == out.pos) {
                retain(old, out);
            }
        }
    }
    Ok(output)
}
fn retain(old: &WordState, new: &mut WordState) {
    let same = old.surface == new.surface
        && old.language == new.language
        && old.morae.len() == new.morae.len()
        && old
            .morae
            .iter()
            .zip(&new.morae)
            .all(|(a, b)| a.reading == b.reading);
    if same {
        let mut kept = old.clone();
        kept.pos = new.pos;
        kept.language = new.language;
        for (a, b) in kept.morae.iter_mut().zip(&new.morae) {
            if !a.accent_edited {
                a.code = b.code;
            }
        }
        *new = kept;
    } else if old.surface == new.surface && old.pronunciation_edited {
        *new = old.clone();
    }
}
pub fn projected_codes(words: &[WordState], segments: &[Segment]) -> Vec<WordState> {
    let mut words = words.to_vec();
    for segment in segments {
        if segment.nucleus {
            if let Some(mora) = words
                .get_mut(segment.token)
                .and_then(|word| word.morae.get_mut(segment.mora))
            {
                mora.code = segment.accent_code;
            }
        }
    }
    words
}
pub fn project(words: &[WordState], segments: &mut [Segment]) {
    for segment in segments {
        if let Some(m) = words
            .get(segment.token)
            .and_then(|w| w.morae.get(segment.mora))
        {
            segment.accent_edited = m.accent_edited;
            segment.intonation_delta = m.intonation_delta;
            segment.high = m.code == roicepeak::phonemes::HIGH;
            segment.accent_code = m.code;
            segment.intonation = m.intonation_delta + segment.raw_query_intonation;
            if let Some(p) = m.phones.get(segment.phone) {
                segment.duration_edit = p.duration;
            }
        }
    }
}
