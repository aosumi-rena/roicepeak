use crate::app::document::{Block, Project, SentenceState};

pub(crate) fn matches(left: &Project, right: &Project) -> bool {
    left.native_origin == right.native_origin
        && left.global == right.global
        && left.narrator_settings == right.narrator_settings
        && left.export_settings == right.export_settings
        && left.blocks.len() == right.blocks.len()
        && left
            .blocks
            .iter()
            .zip(&right.blocks)
            .all(|(a, b)| block(a, b))
}

fn block(a: &Block, b: &Block) -> bool {
    a.id == b.id
        && a.native_owner == b.native_owner
        && a.text == b.text
        && a.narrator == b.narrator
        && a.offset_mode == b.offset_mode
        && a.offset_seconds == b.offset_seconds
        && a.parameters == b.parameters
        && a.sentence_serial == b.sentence_serial
        && a.sentences.len() == b.sentences.len()
        && a.sentences
            .iter()
            .zip(&b.sentences)
            .all(|(a, b)| sentence(a, b))
}

fn sentence(a: &SentenceState, b: &SentenceState) -> bool {
    a.id == b.id
        && a.native_owner == b.native_owner
        && a.text == b.text
        && a.has_eos == b.has_eos
        && a.words == b.words
        && a.word_positions == b.word_positions
        && a.ssml == b.ssml
}
