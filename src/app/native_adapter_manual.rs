//! Native authored words and lossless edits within an existing native tree.
use super::fields::{Field, child_path, get};
use crate::app::document::{
    SentenceState, WordState,
    state::{DurationMode, DurationState, MoraState, PhoneState, WordQueryState},
};
use serde_json::Value;

#[derive(Clone, Debug)]
pub(crate) struct NativeWordRanges {
    pub r8: [i32; 2],
    pub r32: [i32; 2],
}

#[derive(Clone, Debug)]
pub(crate) struct NativeSentenceDraft {
    state: SentenceState,
    pub has_eos: bool,
    pub word_ranges: Vec<NativeWordRanges>,
    original: Value,
}

pub(super) fn decode(value: &Value, id: u64, path: &str) -> Result<NativeSentenceDraft, String> {
    let object = Field { value, path }.object()?;
    let text_path = child_path(path, "text");
    let text = Field {
        value: get(object, "text", path)?,
        path: &text_path,
    }
    .string(crate::app::sentence_edit::MAX_TEXT_BYTES)?
    .to_owned();
    let has_eos_path = child_path(path, "has-eos");
    let has_eos = Field {
        value: get(object, "has-eos", path)?,
        path: &has_eos_path,
    }
    .boolean()?;
    let tokens_path = child_path(path, "tokens");
    let tokens = Field {
        value: get(object, "tokens", path)?,
        path: &tokens_path,
    }
    .array(crate::app::sentence_edit::MAX_WORDS)?;
    let mut words = Vec::with_capacity(tokens.len());
    let mut ranges = Vec::with_capacity(tokens.len());
    let mut budget = crate::app::sentence_edit::ValidationBudget::default();
    let mut mora_count = 0usize;
    let mut phone_count = 0usize;
    for (index, value) in tokens.iter().enumerate() {
        let path = child_path(&tokens_path, &index.to_string());
        let token = Field { value, path: &path }.object()?;
        let field = |key: &str| get(token, key, &path);
        let string = |key: &str, limit| {
            let p = child_path(&path, key);
            Ok::<_, String>(
                Field {
                    value: field(key)?,
                    path: &p,
                }
                .string(limit)?
                .to_owned(),
            )
        };
        let unsigned = |key: &str| {
            let p = child_path(&path, key);
            Field {
                value: field(key)?,
                path: &p,
            }
            .unsigned()
        };
        let native_language = unsigned("lang")?;
        if native_language > 2 {
            return Err(format!(
                "{path}/lang: language has no current canonical word representation"
            ));
        }
        let language = native_language + 1;
        let pe_path = child_path(&path, "pe");
        let pronunciation_edited = Field {
            value: field("pe")?,
            path: &pe_path,
        }
        .boolean()?;
        let r8_path = child_path(&path, "r8");
        let r32_path = child_path(&path, "r32");
        ranges.push(NativeWordRanges {
            r8: Field {
                value: field("r8")?,
                path: &r8_path,
            }
            .pair()?,
            r32: Field {
                value: field("r32")?,
                path: &r32_path,
            }
            .pair()?,
        });
        let syl_path = child_path(&path, "syl");
        let syllables = Field {
            value: field("syl")?,
            path: &syl_path,
        }
        .array(crate::app::sentence_edit::MAX_PHONES)?;
        mora_count = mora_count
            .checked_add(syllables.len())
            .filter(|n| *n <= crate::app::sentence_edit::MAX_PHONES)
            .ok_or_else(|| format!("{syl_path}: sentence mora budget exceeded"))?;
        let mut morae = Vec::with_capacity(syllables.len());
        for (index, value) in syllables.iter().enumerate() {
            let path = child_path(&syl_path, &index.to_string());
            let mora = Field { value, path: &path }.object()?;
            let field = |key: &str| get(mora, key, &path);
            let read_bool = |key: &str| {
                let p = child_path(&path, key);
                Field {
                    value: field(key)?,
                    path: &p,
                }
                .boolean()
            };
            let s_path = child_path(&path, "s");
            let reading = Field {
                value: field("s")?,
                path: &s_path,
            }
            .string(4096)?
            .to_owned();
            let a_path = child_path(&path, "a");
            let code = Field {
                value: field("a")?,
                path: &a_path,
            }
            .unsigned()?;
            roicepeak::phonemes::code_name(code).map_err(|e| format!("{a_path}: {e}"))?;
            let i_path = child_path(&path, "i");
            let intonation_delta = Field {
                value: field("i")?,
                path: &i_path,
            }
            .float32()?;
            let p_path = child_path(&path, "p");
            let native_phones = Field {
                value: field("p")?,
                path: &p_path,
            }
            .array(crate::app::sentence_edit::MAX_PHONES)?;
            phone_count = phone_count
                .checked_add(native_phones.len())
                .filter(|n| *n <= crate::app::sentence_edit::MAX_PHONES)
                .ok_or_else(|| format!("{p_path}: sentence phone budget exceeded"))?;
            let mut phones = Vec::with_capacity(native_phones.len());
            for (index, value) in native_phones.iter().enumerate() {
                let path = child_path(&p_path, &index.to_string());
                let phone = Field { value, path: &path }.object()?;
                let field = |key: &str| get(phone, key, &path);
                let s_path = child_path(&path, "s");
                let symbol = Field {
                    value: field("s")?,
                    path: &s_path,
                }
                .string(4096)?
                .to_owned();
                let n_path = child_path(&path, "n");
                let raw_nucleus = Field {
                    value: field("n")?,
                    path: &n_path,
                }
                .boolean()?;
                let dt_path = child_path(&path, "dt");
                let mode = match (Field {
                    value: field("dt")?,
                    path: &dt_path,
                })
                .signed()?
                {
                    0 => DurationMode::Relative,
                    1 => DurationMode::Absolute,
                    _ => return Err(format!("{dt_path}: unsupported duration mode")),
                };
                let d_path = child_path(&path, "d");
                let duration = DurationState {
                    mode,
                    value: Field {
                        value: field("d")?,
                        path: &d_path,
                    }
                    .float64()?,
                };
                if !duration.valid() {
                    return Err(format!("{d_path}: negative authored duration"));
                }
                phones.push(PhoneState {
                    nucleus: if language == 1 {
                        roicepeak::japanese::is_nucleus(&symbol)
                    } else {
                        raw_nucleus
                    },
                    symbol,
                    duration,
                });
            }
            morae.push(MoraState {
                reading,
                code,
                prediction_eligible: read_bool("ig")?,
                accent_edited: read_bool("u")?,
                intonation_delta,
                phones,
            });
        }
        let word = WordState {
            surface: string("s", crate::app::sentence_edit::MAX_TEXT_BYTES)?,
            language,
            pos: unsigned("pos")?,
            pronunciation_edited,
            morae,
        };
        crate::app::sentence_edit::validate_word(&word, &mut budget)
            .map_err(|e| format!("{path}: {e}"))?;
        words.push(word);
    }
    Ok(NativeSentenceDraft {
        state: SentenceState {
            id,
            native_owner: None,
            text,
            words,
            word_positions: Default::default(),
            query: WordQueryState::default(),
            ssml: None,
        },
        has_eos,
        word_ranges: ranges,
        original: value.clone(),
    })
}

impl NativeSentenceDraft {
    pub fn state(&self) -> &SentenceState {
        &self.state
    }
    /// Update authored values while keeping native ranges and opaque properties.
    /// Structural/text edits require the separate origin-projection adapter.
    pub fn preserves_structure(&self, words: &[WordState]) -> Result<bool, String> {
        let source = &self.state.words;
        let boundary = |word: &WordState| matches!(word.pos, 0x2000 | 0x2001);
        if source.iter().any(boundary) {
            return Err("Native source contains an unqualified boundary-position word".into());
        }
        let mut ordinary = Vec::with_capacity(words.len());
        for word in words {
            if boundary(word) {
                if word.manual() {
                    return Err(
                        "Authored synthetic boundary needs an explicit native storage mapping"
                            .into(),
                    );
                }
            } else {
                ordinary.push(word);
            }
        }
        let words = ordinary;
        if source.len() != words.len() {
            return Ok(false);
        }
        let mut budget = crate::app::sentence_edit::ValidationBudget::default();
        for (a, b) in source.iter().zip(&words) {
            crate::app::sentence_edit::validate_word(b, &mut budget)?;
            if a.surface != b.surface
                || a.language != b.language
                || a.pos != b.pos
                || a.morae.len() != b.morae.len()
                || a.morae.iter().zip(&b.morae).any(|(a, b)| {
                    a.reading != b.reading
                        || a.phones.len() != b.phones.len()
                        || a.phones
                            .iter()
                            .zip(&b.phones)
                            .any(|(a, b)| a.symbol != b.symbol || a.nucleus != b.nucleus)
                })
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn overlay_manual(&self, words: &[WordState]) -> Result<Value, String> {
        if !self.preserves_structure(words)? {
            return Err("Native word structure changed; origin projection is required".into());
        }
        let source = &self.state.words;
        let words = words
            .iter()
            .filter(|word| !matches!(word.pos, 0x2000 | 0x2001))
            .collect::<Vec<_>>();
        let mut value = self.original.clone();
        let tokens = value["tokens"].as_array_mut().unwrap();
        for ((source_word, word), target) in source.iter().zip(&words).zip(tokens) {
            let source = source_word;
            if source.pronunciation_edited != word.pronunciation_edited {
                target["pe"] = word.pronunciation_edited.into();
            }
            let syllables = target["syl"].as_array_mut().unwrap();
            for ((source, mora), target) in source.morae.iter().zip(&word.morae).zip(syllables) {
                let authored_code = source.accent_edited
                    || mora.accent_edited
                    || source_word.pronunciation_edited != word.pronunciation_edited;
                if authored_code && source.code != mora.code {
                    target["a"] = mora.code.into();
                }
                if source.prediction_eligible != mora.prediction_eligible {
                    target["ig"] = mora.prediction_eligible.into();
                }
                if source.accent_edited != mora.accent_edited {
                    target["u"] = mora.accent_edited.into();
                }
                if source.intonation_delta.to_bits() != mora.intonation_delta.to_bits() {
                    target["i"] = Value::from(f64::from(mora.intonation_delta));
                }
                let phones = target["p"].as_array_mut().unwrap();
                for ((source, phone), target) in source.phones.iter().zip(&mora.phones).zip(phones)
                {
                    if source.duration.mode != phone.duration.mode {
                        target["dt"] = match phone.duration.mode {
                            DurationMode::Relative => 0,
                            DurationMode::Absolute => 1,
                        }
                        .into();
                    }
                    if source.duration.value.to_bits() != phone.duration.value.to_bits() {
                        target["d"] = Value::from(phone.duration.value);
                    }
                    // Raw native n is preserved: derived Japanese query flags are
                    // not authored changes to the serialized constructor state.
                }
            }
        }
        Ok(value)
    }
}
