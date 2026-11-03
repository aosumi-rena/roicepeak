use super::mora::Word;
use crate::app::{
    document::{Block, state},
    editor::Mode,
    i18n::{Locale, tr},
};
use eframe::egui::{self, Rect, Sense};

use crate::app::native_menu as menu;

#[derive(Clone, Copy)]
enum Kind {
    Reading,
    Duration,
    Intonation,
    Accent,
}

pub(super) fn word(
    ui: &mut egui::Ui,
    block: &mut Block,
    word: &Word,
    rect: Rect,
    mode: Mode,
    enabled: bool,
    locale: Locale,
) -> bool {
    let id = ui
        .id()
        .with(("word-menu", block.id, word.sentence_id, word.token));
    let anchor = ui.interact(rect, id.with("anchor"), Sense::hover());
    let open = (ui.rect_contains_pointer(rect) && ui.input(|i| i.pointer.secondary_clicked()))
        .then_some(egui::SetOpenCommand::Bool(true));
    let lane = match mode {
        Mode::Accent => Kind::Accent,
        Mode::Intonation => Kind::Intonation,
        Mode::Duration => Kind::Duration,
    };
    let actions = [
        (Kind::Reading, false),
        (lane, false),
        (Kind::Reading, true),
        (lane, true),
    ];
    let rows = actions.map(|(kind, all)| {
        let editable = if all {
            block
                .sentences
                .iter()
                .any(|s| s.words.iter().any(|w| edited(w, kind)))
        } else {
            block
                .sentences
                .get(word.sentence_index)
                .filter(|s| s.id == word.sentence_id)
                .and_then(|s| s.words.get(word.token))
                .is_some_and(|w| edited(w, kind))
        };
        let (ja, en) = label(kind, all);
        menu::Row {
            text: tr(locale, ja, en),
            enabled: enabled && editable,
        }
    });
    let Some(index) = menu::show(ui, &anchor, id, open, &rows) else {
        return false;
    };
    let (kind, all) = actions[index];
    let mut changed = false;
    for (sentence_index, sentence) in block.sentences.iter_mut().enumerate() {
        if !all && (sentence_index != word.sentence_index || sentence.id != word.sentence_id) {
            continue;
        }
        if sentence.query.text != sentence.text
            || (all && !sentence.query.matches(&sentence.text, &sentence.words))
        {
            continue;
        }
        for (token, current) in sentence.words.iter_mut().enumerate() {
            if !all && token != word.token {
                continue;
            }
            if !sentence
                .query
                .controlled
                .get(token)
                .is_some_and(|query| current.matches_query(query))
            {
                continue;
            }
            let basis = match kind {
                Kind::Reading => &sentence.query.raw,
                Kind::Accent if all => &sentence.query.pronunciation,
                Kind::Accent => &sentence.query.predicted,
                _ => &sentence.query.controlled,
            };
            changed |= reset(current, basis.get(token), kind);
        }
    }
    if !changed {
        return false;
    }
    if matches!(kind, Kind::Reading) {
        block.segments.clear();
    } else {
        for segment in &mut block.segments {
            if let Some(sentence) = block.sentences.get(segment.sentence.index) {
                state::project(&sentence.words, std::slice::from_mut(segment));
            }
        }
    }
    matches!(kind, Kind::Reading | Kind::Accent)
}

fn reset(current: &mut state::WordState, baseline: Option<&state::WordState>, kind: Kind) -> bool {
    match kind {
        Kind::Reading => {
            return baseline.is_some_and(|baseline| current.reset_reading(baseline));
        }
        Kind::Duration => {
            let changed = current.morae.iter().any(|m| {
                m.phones.iter().any(|p| {
                    state::duration_phone(current.language, &p.symbol, p.nucleus)
                        && (p.duration.mode != state::DurationMode::Relative
                            || p.duration.value != 1.0)
                })
            });
            current.reset_duration();
            return changed;
        }
        Kind::Intonation => {
            let changed = edited(current, kind);
            current.reset_intonation();
            return changed;
        }
        Kind::Accent => {
            if !edited(current, kind) {
                return false;
            }
            let Some(baseline) =
                baseline.filter(|baseline| current.morae.len() == baseline.morae.len())
            else {
                return false;
            };
            for (index, mora) in current.morae.iter_mut().enumerate() {
                mora.code = baseline.morae[index].code;
                mora.accent_edited = false;
            }
        }
    }
    true
}

fn edited(word: &state::WordState, kind: Kind) -> bool {
    match kind {
        Kind::Reading => word.pronunciation_edited,
        Kind::Duration => word.morae.iter().any(|m| {
            m.phones.iter().any(|p| {
                state::duration_phone(word.language, &p.symbol, p.nucleus)
                    && p.duration.value != 1.0
            })
        }),
        Kind::Intonation => word.morae.iter().any(|m| m.intonation_delta != 0.0),
        Kind::Accent => word.morae.iter().any(|m| m.accent_edited),
    }
}

fn label(kind: Kind, all: bool) -> (&'static str, &'static str) {
    match (kind, all) {
        (Kind::Reading, false) => ("読みをリセット", "Reset Pronunciation"),
        (Kind::Duration, false) => ("長さをリセット", "Reset Duration"),
        (Kind::Intonation, false) => ("イントネーションをリセット", "Reset Intonation"),
        (Kind::Accent, false) => ("アクセントをリセット", "Reset Accent"),
        (Kind::Reading, true) => ("すべての読みをリセット", "Reset All Pronunciations"),
        (Kind::Duration, true) => ("すべての長さをリセット", "Reset All Durations"),
        (Kind::Intonation, true) => (
            "すべてのイントネーションをリセット",
            "Reset All Intonations",
        ),
        (Kind::Accent, true) => ("すべてのアクセントをリセット", "Reset All Accents"),
    }
}
