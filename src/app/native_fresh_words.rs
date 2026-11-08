use crate::app::{
    document::{SentenceState, WordState, state::DurationMode},
    sentence_edit::{ValidationBudget, validate_word},
};
use serde_json::{Value, json};

fn boundary(word: &WordState) -> bool {
    matches!(word.pos, 0x2000 | 0x2001)
}

fn endpoints(first: usize, last: usize) -> Result<Value, String> {
    Ok(json!([
        i32::try_from(first).map_err(|_| "Native position exceeds int32")?,
        i32::try_from(last).map_err(|_| "Native position exceeds int32")?,
    ]))
}

pub(super) fn replacement(sentence: &SentenceState) -> Result<Value, String> {
    if sentence.ssml.is_some() {
        return Err("Native Save cannot preserve pending pronunciation".into());
    }
    if sentence.word_positions.source != sentence.text
        || sentence.word_positions.ranges.len() != sentence.words.len()
    {
        return Err("Native Save requires current word positions".into());
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
        if boundary(word) {
            if word.manual() || sentence.word_positions.ranges[index].is_some() {
                return Err("An authored boundary has no native word representation".into());
            }
            continue;
        }
        if word.language != 1 || word.pos > i32::MAX as u32 {
            return Err("Native Save requires Japanese authored words".into());
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
        for mora in &word.morae {
            let code = mora.code;
            roicepeak::phonemes::code_name(code).map_err(|e| e.to_string())?;
            let phones = mora
                .phones
                .iter()
                .map(|phone| {
                    json!({"s":phone.symbol,"dt":match phone.duration.mode {
                    DurationMode::Relative=>0,DurationMode::Absolute=>1,
                },"d":phone.duration.value,"n":phone.stored_nucleus})
                })
                .collect::<Vec<_>>();
            morae.push(
                json!({"s":mora.reading,"a":code,"ig":mora.prediction_eligible,
                "u":mora.accent_edited,"i":f64::from(mora.intonation_delta),"p":phones}),
            );
        }
        tokens.push(
            json!({"s":word.surface,"pos":word.pos,"lang":0,"pe":word.pronunciation_edited,
            "syl":morae,"r8":endpoints(first,last)?,"r32":endpoints(range.start,range.end)?}),
        );
    }
    Ok(json!({"text":sentence.text,"has-eos":sentence.has_eos,"tokens":tokens}))
}
