use super::{Entry, POS};
use crate::app::{dialog_controls, editor::EditorApp, i18n, style};
use eframe::egui::{self, Color32, Id, Pos2, Rect, Stroke, Vec2};

#[path = "dictionary_accent.rs"]
mod accent;
#[path = "dictionary_priority.rs"]
mod priority;
#[path = "dictionary_reading.rs"]
mod reading;
#[path = "dictionary_scroll.rs"]
mod scroll;
#[path = "dictionary_text.rs"]
mod text;

const INPUT_SIZE: f32 = 23.04 / 1.48;

#[derive(Default)]
pub(super) struct State {
    word: String,
    reading: String,
    reading_editing: bool,
    reading_focus: bool,
    restore_reading: bool,
    accent: accent::State,
    word_composition: text::Composition,
    reading_composition: text::Composition,
    scroll: scroll::State,
    priority: priority::State,
}

impl State {
    pub(super) fn begin(&mut self, entry: &Entry) {
        *self = Self {
            word: entry.word.clone(),
            reading: entry.reading.clone(),
            ..Self::default()
        };
    }

    fn commit_reading(&mut self, entry: &mut Entry) -> Result<(), String> {
        let (normalized, _) = reading::parse(&self.reading)?;
        if normalized != entry.reading {
            entry.reading = normalized;
            entry.accent = 0;
            entry.native.overwrite_accents.clear();
            self.accent.reset();
        }
        self.reading_composition.cancel();
        self.reading = entry.reading.clone();
        self.reading_editing = false;
        self.reading_focus = false;
        Ok(())
    }
}

pub(super) fn show(app: &mut EditorApp, ui: &mut egui::Ui) -> bool {
    let Some(mut entry) = app.dictionary_ui.draft.take() else {
        return false;
    };
    let dictionary = app.dictionary_state.display_snapshot();
    let writable = app.dictionary_writable();
    let locale = app.locale;
    let origin = ui.max_rect().min;
    let mut save = false;
    let mut cancel = false;
    ui.add_enabled_ui(app.dictionary_ui.validation.is_none(), |ui| {
        let state = &mut app.dictionary_ui.draft_state;
        card(ui, origin);
        word(ui, origin, &mut entry, state, locale);
        if let Err(error) = pronunciation(ui, origin, &mut entry, state, locale) {
            state.restore_reading = true;
            app.dictionary_ui.validation = Some(i18n::text(locale, &error).to_owned());
        }
        properties(ui, origin, &mut entry, &mut state.priority, locale);
        save = style::text_button(
            ui,
            Rect::from_min_size(origin + Vec2::new(208.0, 344.0), Vec2::new(100.0, 32.0)),
            i18n::tr(locale, "保存", "Save"),
            true,
            writable,
        )
        .clicked();
        cancel = style::text_button(
            ui,
            Rect::from_min_size(origin + Vec2::new(316.0, 344.0), Vec2::new(100.0, 32.0)),
            i18n::tr(locale, "キャンセル", "Cancel"),
            false,
            true,
        )
        .clicked();
    });
    if cancel {
        app.dictionary_ui.validation = None;
        app.dictionary_ui.draft_state = State::default();
        return false;
    }
    if save
        && app.dictionary_ui.validation.is_none()
        && !app.dictionary_ui.draft_state.word_composition.busy()
        && !app.dictionary_ui.draft_state.reading_composition.busy()
    {
        let state = &mut app.dictionary_ui.draft_state;
        entry.word = state.word.clone();
        if state.reading_editing {
            if let Err(error) = state.commit_reading(&mut entry) {
                state.restore_reading = true;
                app.dictionary_ui.validation = Some(i18n::text(locale, &error).to_owned());
            }
        }
        if app.dictionary_ui.validation.is_none() {
            if let Some(error) = save_error(&entry) {
                app.dictionary_ui.validation = Some(i18n::text(locale, error).to_owned());
            } else {
                let mut stored = entry.clone();
                stored.native.opaque = false;
                stored.word = stored.word.trim().into();
                stored.reading = stored.reading.trim().into();
                if let Err(error) = stored.validate_stored() {
                    app.dictionary_ui.validation = Some(error);
                } else {
                    let mut entries = dictionary.entries().to_vec();
                    let selected = if let Some(index) =
                        app.dictionary_ui.editing.filter(|&i| i < entries.len())
                    {
                        entries[index] = stored;
                        index
                    } else if entries.len() < 10_000 {
                        let index = entries.len();
                        entries.push(stored);
                        index
                    } else {
                        app.dictionary_ui.validation = Some("Dictionary entry limit".into());
                        app.dictionary_ui.draft = Some(entry);
                        return true;
                    };
                    match app.commit_dictionary(&dictionary, entries) {
                        Ok(()) => {
                            app.dictionary_ui.selected = Some(selected);
                            return false;
                        }
                        Err(error) => app.dictionary_ui.validation = Some(error),
                    }
                }
            }
        }
    }
    if let Some(message) = app.dictionary_ui.validation.clone() {
        if crate::app::menus::native_alert::show(
            ui.ctx(),
            ui.id().with("dictionary-validation"),
            "ROICEPEAK",
            &message,
            crate::app::menus::native_alert::Kind::Error,
            &["OK"],
        )
        .is_some()
        {
            app.dictionary_ui.validation = None;
            let state = &mut app.dictionary_ui.draft_state;
            if state.restore_reading {
                state.reading_composition.cancel();
                state.reading = entry.reading.clone();
                state.reading_editing = false;
                state.reading_focus = false;
                state.restore_reading = false;
            }
        }
    }
    app.dictionary_ui.draft = Some(entry);
    true
}

fn save_error(entry: &Entry) -> Option<&'static str> {
    if entry.word.trim().is_empty() {
        Some("Please set the spelling.")
    } else if entry.word.contains('"') {
        Some("Words containing double quotations cannot be registered.")
    } else if entry.reading.trim().is_empty() {
        Some("Please set the pronunciation.")
    } else {
        None
    }
}

fn card(ui: &egui::Ui, origin: Pos2) {
    let card = Rect::from_min_size(origin + Vec2::new(16.0, 16.0), Vec2::new(400.0, 214.0));
    ui.painter().rect_stroke(
        card,
        3,
        Stroke::new(1.0, style::BORDER),
        egui::StrokeKind::Inside,
    );
    ui.painter().line_segment(
        [
            origin + Vec2::new(16.0, 76.0),
            origin + Vec2::new(416.0, 76.0),
        ],
        Stroke::new(1.0, style::BORDER),
    );
}

fn caption(ui: &egui::Ui, center: Pos2, text: &str) {
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        text,
        style::font(14.0),
        Color32::from_gray(178),
    );
}

fn field(ui: &mut egui::Ui, rect: Rect, id: Id, text: &mut String) -> egui::Response {
    let response = ui.put(
        rect,
        egui::TextEdit::singleline(text)
            .id(id)
            .char_limit(4096)
            .desired_width(rect.width())
            .font(style::font(INPUT_SIZE))
            .text_color(style::TEXT)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::symmetric(12, 0))
            .horizontal_align(egui::Align::Center)
            .vertical_align(egui::Align::Center),
    );
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            3,
            Stroke::new(1.0, Color32::from_rgb(40, 131, 235)),
            egui::StrokeKind::Inside,
        );
    }
    response
}

fn measured(ui: &egui::Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), style::font(INPUT_SIZE), style::TEXT)
        .size()
        .x
        .ceil()
        + 24.0
}

fn word(
    ui: &mut egui::Ui,
    origin: Pos2,
    entry: &mut Entry,
    state: &mut State,
    locale: i18n::Locale,
) {
    caption(
        ui,
        origin + Vec2::new(216.0, 32.0),
        i18n::tr(locale, "単語", "Word"),
    );
    let id = ui.id().with("dictionary-word");
    let was_focused = ui.memory(|m| m.has_focus(id));
    let rect = Rect::from_center_size(
        origin + Vec2::new(216.0, 58.0),
        Vec2::new(measured(ui, &state.word), 28.0),
    );
    let (escape, enter) = state.word_composition.keys(ui, was_focused);
    let response = field(ui, rect, id, &mut state.word);
    if !ui.is_enabled() {
        return;
    }
    if (was_focused || response.has_focus()) && escape {
        state.word = entry.word.clone();
        state.word_composition.cancel();
        response.surrender_focus();
    } else if state.word_composition.commit(response.lost_focus(), enter) {
        entry.word = state.word.clone();
        response.surrender_focus();
    }
}

fn pronunciation(
    ui: &mut egui::Ui,
    origin: Pos2,
    entry: &mut Entry,
    state: &mut State,
    locale: i18n::Locale,
) -> Result<(), String> {
    caption(
        ui,
        origin + Vec2::new(216.0, 92.0),
        i18n::tr(locale, "発音 / アクセント", "Pronunciation / Accent"),
    );
    let parsed = roicepeak::japanese::edit_reading(&entry.reading);
    let moras = parsed.as_ref().map(|v| v.len()).unwrap_or(0);
    let viewport = Rect::from_min_size(origin + Vec2::new(28.0, 104.0), Vec2::new(376.0, 118.0));
    let width = viewport.width().max((moras + 1) as f32 * accent::CELL);
    let mut scroll = std::mem::take(&mut state.scroll);
    let scrolled = scroll.show(ui, viewport, width, |ui, content| {
        let label_row = Rect::from_min_size(content.min, Vec2::new(width, 28.0));
        if !state.reading_editing {
            let response = ui.interact(
                label_row,
                ui.id().with("dictionary-pronunciation-button"),
                egui::Sense::click(),
            );
            if let Ok(morae) = &parsed {
                let left = label_row.center().x - (morae.len() + 1) as f32 * accent::CELL * 0.5;
                for i in 0..=morae.len() {
                    let text = morae.get(i).map(|m| m.reading.as_str()).unwrap_or("（は）");
                    ui.painter().with_clip_rect(viewport).text(
                        Pos2::new(left + (i as f32 + 0.5) * accent::CELL, label_row.center().y),
                        egui::Align2::CENTER_CENTER,
                        text,
                        style::font(INPUT_SIZE),
                        style::TEXT,
                    );
                }
            } else {
                ui.painter().with_clip_rect(viewport).text(
                    label_row.center(),
                    egui::Align2::CENTER_CENTER,
                    &entry.reading,
                    style::font(INPUT_SIZE),
                    style::TEXT,
                );
            }
            if response.clicked() {
                state.reading_composition.cancel();
                state.reading = entry.reading.clone();
                state.reading_editing = true;
                state.reading_focus = true;
            }
        }
        if parsed.is_ok() {
            let accent_rect = Rect::from_min_max(content.min + Vec2::new(0.0, 28.0), content.max);
            state.accent.show(ui, accent_rect, entry, moras)?;
        }
        Ok::<(), String>(())
    });
    state.scroll = scroll;
    scrolled?;
    if state.reading_editing {
        let id = ui.id().with("dictionary-reading");
        let was_focused = ui.memory(|m| m.has_focus(id));
        let (escape, enter) = state
            .reading_composition
            .keys(ui, was_focused || state.reading_focus);
        let width = measured(ui, &state.reading)
            .max((moras + 1) as f32 * accent::CELL)
            .min(viewport.width());
        let response = field(
            ui,
            Rect::from_center_size(origin + Vec2::new(216.0, 118.0), Vec2::new(width, 28.0)),
            id,
            &mut state.reading,
        );
        if state.reading_focus {
            response.request_focus();
            state.reading_focus = false;
        }
        if !ui.is_enabled() {
            return Ok(());
        }
        if (was_focused || response.has_focus()) && escape {
            state.reading_composition.cancel();
            state.reading = entry.reading.clone();
            state.reading_editing = false;
            response.surrender_focus();
        } else if state
            .reading_composition
            .commit(response.lost_focus(), enter)
        {
            response.surrender_focus();
            state.commit_reading(entry)?;
        }
    }
    Ok(())
}

fn properties(
    ui: &mut egui::Ui,
    origin: Pos2,
    entry: &mut Entry,
    priority: &mut priority::State,
    locale: i18n::Locale,
) {
    style::label(
        ui,
        origin + Vec2::new(20.0, 265.0),
        i18n::tr(locale, "品詞", "Part of speech"),
        14.0,
        style::TEXT,
    );
    let labels = POS.map(|text| i18n::text(locale, text));
    let choices = labels
        .iter()
        .enumerate()
        .map(|(i, text)| (i, *text))
        .collect::<Vec<_>>();
    dialog_controls::combo(
        ui,
        Rect::from_min_size(origin + Vec2::new(128.0, 249.0), Vec2::new(288.0, 32.0)),
        ui.id().with("dictionary-pos"),
        &mut entry.part_of_speech,
        &choices,
    );
    style::label(
        ui,
        origin + Vec2::new(20.0, 311.0),
        i18n::tr(locale, "優先度", "Priority"),
        14.0,
        style::TEXT,
    );
    priority.show(
        ui,
        Rect::from_min_size(origin + Vec2::new(128.0, 297.0), Vec2::new(288.0, 28.0)),
        &mut entry.priority,
        locale,
    );
}
