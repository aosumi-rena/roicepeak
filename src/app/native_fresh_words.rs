use crate::app::{
    document::{SentenceState, WordState, state::DurationMode},
    sentence_edit::{ValidationBudget, validate_word},
};
use serde_json::{Value, json};

fn boundary(word: &WordState) -> bool {
    matches!(word.pos, 0x2000 | 0x2001)
}

fn topology(a: &WordState, b: &WordState) -> bool {
    a.surface == b.surface
        && a.language == b.language
        && a.pos == b.pos
        && a.morae.len() == b.morae.len()
        && a.morae.iter().zip(&b.morae).all(|(a, b)| {
            a.reading == b.reading
                && a.phones.len() == b.phones.len()
                && a.phones
                    .iter()
                    .zip(&b.phones)
                    .all(|(a, b)| a.symbol == b.symbol)
        })
}

fn endpoints(first: usize, last: usize) -> Result<Value, String> {
    Ok(json!([
        i32::try_from(first).map_err(|_| "Native position exceeds int32")?,
        i32::try_from(last).map_err(|_| "Native position exceeds int32")?,
    ]))
}

pub(super) fn sentence(sentence: &SentenceState) -> Result<Value, String> {
    if sentence.native_owner.is_some() {
        return Err("A new native sentence cannot reuse a native owner".into());
    }
    replacement(sentence)
}

pub(super) fn replacement(sentence: &SentenceState) -> Result<Value, String> {
    if sentence.ssml.is_some() {
        return Err("Native Save cannot preserve pending pronunciation".into());
    }
    let query = &sentence.query;
    if sentence.text.is_empty()
        || query.text != sentence.text
        || sentence.word_positions.source != sentence.text
        || query
            .snapshots()
            .iter()
            .any(|words| words.len() != sentence.words.len())
        || sentence.word_positions.ranges.len() != sentence.words.len()
    {
        return Err("Prepare the current text before saving a new native sentence".into());
    }
    sentence.word_positions.validate(&sentence.words)?;
    let bytes = sentence
        .text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(sentence.text.len()))
        .collect::<Vec<_>>();
    let mut budget = ValidationBudget::default();
    let mut tokens = Vec::new();
    let mut previous = (0, 0);
    for (index, word) in sentence.words.iter().enumerate() {
        validate_word(word, &mut budget)?;
        let raw = &query.raw[index];
        let pronunciation = &query.pronunciation[index];
        let predicted = &query.predicted[index];
        let controlled = &query.controlled[index];
        for snapshot in [raw, pronunciation, predicted, controlled] {
            if snapshot.surface != word.surface
                || snapshot.language != word.language
                || snapshot.pos != word.pos
            {
                return Err("Native Save requires the current analyzed word identities".into());
            }
        }
        if boundary(word) {
            if word.manual() || sentence.word_positions.ranges[index].is_some() {
                return Err("An authored boundary has no native word representation".into());
            }
            continue;
        }
        if word.language != 1
            || word.pos > i32::MAX as u32
            || !topology(word, pronunciation)
            || !topology(word, predicted)
            || !topology(word, controlled)
            || word.pronunciation_edited != pronunciation.pronunciation_edited
            || word.pronunciation_edited != predicted.pronunciation_edited
        {
            return Err("Prepare the current Japanese pronunciation before Native Save".into());
        }
        let range = sentence.word_positions.ranges[index]
            .as_ref()
            .ok_or("Native Save requires an analyzed range for each word")?;
        if range.start < previous.0 || range.end < previous.1 {
            return Err("Native word ranges are not in source order".into());
        }
        previous = (range.start, range.end);
        let first = *bytes
            .get(range.start)
            .ok_or("Native word start is outside the text")?;
        let last = *bytes
            .get(range.end)
            .ok_or("Native word end is outside the text")?;
        let mut morae = Vec::with_capacity(word.morae.len());
        for (mora, baseline) in word.morae.iter().zip(&predicted.morae) {
            if mora.prediction_eligible != baseline.prediction_eligible {
                return Err("Prepare the current accent before Native Save".into());
            }
            let code = if mora.accent_edited {
                mora.code
            } else {
                baseline.code
            };
            roicepeak::phonemes::code_name(code).map_err(|e| e.to_string())?;
            let phones = mora
                .phones
                .iter()
                .map(|phone| {
                    let nucleus = roicepeak::japanese::is_nucleus(&phone.symbol);
                    if phone.nucleus != nucleus {
                        return Err(
                            "Japanese phone flags do not match the prepared pronunciation"
                                .to_owned(),
                        );
                    }
                    Ok(json!({"s":phone.symbol,"dt":match phone.duration.mode {
                    DurationMode::Relative=>0,DurationMode::Absolute=>1,
                },"d":phone.duration.value,"n":nucleus}))
                })
                .collect::<Result<Vec<_>, String>>()?;
            morae.push(
                json!({"s":mora.reading,"a":code,"ig":baseline.prediction_eligible,
                "u":mora.accent_edited,"i":f64::from(mora.intonation_delta),"p":phones}),
            );
        }
        tokens.push(
            json!({"s":word.surface,"pos":word.pos,"lang":0,"pe":word.pronunciation_edited,
            "syl":morae,"r8":endpoints(first,last)?,"r32":endpoints(range.start,range.end)?}),
        );
    }
    Ok(json!({"text":sentence.text,"has-eos":true,"tokens":tokens}))
}
