use crate::app::document::{Project, WordState};

fn words(left: &WordState, right: &WordState) -> bool {
    left.surface == right.surface
        && left.language == right.language
        && left.pos == right.pos
        && left.pronunciation_edited == right.pronunciation_edited
        && left.morae.len() == right.morae.len()
        && left.morae.iter().zip(&right.morae).all(|(a, b)| {
            a.reading == b.reading
                && a.accent_edited == b.accent_edited
                && (!left.pronunciation_edited || a.prediction_eligible == b.prediction_eligible)
                && (!(a.accent_edited || left.pronunciation_edited) || a.code == b.code)
                && a.phones.len() == b.phones.len()
                && a.phones
                    .iter()
                    .zip(&b.phones)
                    .all(|(a, b)| a.symbol == b.symbol && a.nucleus == b.nucleus)
        })
}

pub(crate) fn matches(left: &Project, right: &Project) -> bool {
    left.global.emotions == right.global.emotions
        && left.narrator_settings.iter().all(|(key, value)| {
            right
                .narrator_settings
                .get(key)
                .map_or(value.emotions.is_empty(), |other| {
                    value.emotions == other.emotions
                })
        })
        && right.narrator_settings.iter().all(|(key, value)| {
            left.narrator_settings.contains_key(key) || value.emotions.is_empty()
        })
        && left.blocks.len() == right.blocks.len()
        && left.blocks.iter().zip(&right.blocks).all(|(a, b)| {
            a.id == b.id
                && a.text == b.text
                && a.narrator == b.narrator
                && a.parameters.emotions == b.parameters.emotions
                && a.sentences.len() == b.sentences.len()
                && a.sentences.iter().zip(&b.sentences).all(|(a, b)| {
                    a.id == b.id
                        && a.text == b.text
                        && a.ssml == b.ssml
                        && a.words.len() == b.words.len()
                        && a.words.iter().zip(&b.words).all(|(a, b)| words(a, b))
                })
        })
}
