use super::{Lane, Target};
use crate::app::document::{Project, state};

pub(super) fn only_lane(before: &Project, after: &Project, target: Target) -> bool {
    let Some(block_index) = before.blocks.iter().position(|b| b.id == target.block) else {
        return false;
    };
    let original_block = &before.blocks[block_index];
    let Some(sentence_index) = original_block
        .sentences
        .iter()
        .position(|s| s.id == target.sentence)
    else {
        return false;
    };
    let original = &original_block.sentences[sentence_index];
    let mut normalized = after.clone();
    let Some(block) = normalized
        .blocks
        .get_mut(block_index)
        .filter(|b| b.id == target.block)
    else {
        return false;
    };
    let Some(sentence) = block
        .sentences
        .get_mut(sentence_index)
        .filter(|s| s.id == target.sentence)
    else {
        return false;
    };
    if sentence.words.len() != original.words.len() {
        return false;
    }
    for (a, b) in original.words.iter().zip(&mut sentence.words) {
        if a.morae.len() != b.morae.len() {
            return false;
        }
        for (a_mora, b_mora) in a.morae.iter().zip(&mut b.morae) {
            match target.lane {
                Lane::Accent => {
                    b_mora.code = a_mora.code;
                    b_mora.accent_edited = a_mora.accent_edited;
                }
                Lane::Intonation => b_mora.intonation_delta = a_mora.intonation_delta,
                Lane::Duration => {
                    if a_mora.phones.len() != b_mora.phones.len() {
                        return false;
                    }
                    for (a_phone, b_phone) in a_mora.phones.iter().zip(&mut b_mora.phones) {
                        if state::duration_phone(a.language, &a_phone.symbol, a_phone.nucleus) {
                            b_phone.duration = a_phone.duration;
                        }
                    }
                }
            }
        }
    }
    if original_block.segments.len() != block.segments.len() {
        return false;
    }
    for (a, b) in original_block.segments.iter().zip(&mut block.segments) {
        if a.sentence.index != sentence_index {
            continue;
        }
        if a.sentence != b.sentence || a.token != b.token || a.mora != b.mora || a.phone != b.phone
        {
            return false;
        }
        match target.lane {
            Lane::Accent => {
                b.high = a.high;
                b.accent_code = a.accent_code;
                b.accent_edited = a.accent_edited;
            }
            Lane::Intonation => {
                b.intonation = a.intonation;
                b.intonation_delta = a.intonation_delta;
            }
            Lane::Duration => {
                b.duration = a.duration;
                b.duration_edit = a.duration_edit;
            }
        }
    }
    before.same_content(&normalized)
}
