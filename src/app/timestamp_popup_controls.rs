use super::{Draft, number, paint};
use crate::app::style;
use eframe::egui::{self, Color32, Id, Rect, Sense, Vec2};

pub(super) fn segment(
    ui: &mut egui::Ui,
    rect: Rect,
    id: Id,
    text: &str,
    selected: bool,
    enabled: bool,
    color: Color32,
    left: bool,
) -> bool {
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let mut fill = if selected {
        color
    } else {
        Color32::from_black_alpha(26)
    };
    let mut text_color = if selected {
        Color32::WHITE
    } else {
        style::TEXT
    };
    if !enabled {
        let mut hsv = egui::ecolor::Hsva::from(fill);
        hsv.s *= 0.5;
        fill = Color32::from(hsv);
        text_color = text_color.gamma_multiply(0.5);
    }
    if enabled {
        fill = paint::interactive_color(fill, &response);
    }
    let radius = if left {
        egui::CornerRadius {
            nw: 4,
            sw: 4,
            ne: 0,
            se: 0,
        }
    } else {
        egui::CornerRadius {
            nw: 0,
            sw: 0,
            ne: 4,
            se: 4,
        }
    };
    ui.painter().rect_filled(rect, radius, fill);
    ui.painter()
        .with_clip_rect(rect.shrink2(Vec2::new(4.0, 2.0)))
        .text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            style::font(20.880_001 / 1.48),
            text_color,
        );
    response.clicked() && enabled
}

pub(super) fn normalized(text: &str) -> String {
    text.chars()
        .filter(|c| "+-0123456789.".contains(*c))
        .take(12)
        .collect()
}

pub(super) fn value(ui: &mut egui::Ui, rect: Rect, draft: &mut Draft) {
    let id = Id::new(("timestamp-value", draft.block));
    if !ui.is_enabled() {
        draft.edit = None;
        egui::text_edit::TextEditState::default().store(ui.ctx(), id);
        ui.memory_mut(|m| m.surrender_focus(id));
    }
    let surface = ui.interact(
        rect,
        id.with("surface"),
        if draft.edit.is_none() && ui.is_enabled() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let hovered = surface.hovered();
    let starting = draft.edit.is_none() && surface.is_pointer_button_down_on() && ui.is_enabled();
    let window_focused = ui.input(|i| i.focused);
    let focused_before = window_focused && ui.memory(|m| m.has_focus(id));
    paint::field_background(
        ui,
        rect,
        starting || (draft.edit.is_some() && focused_before),
        hovered,
    );
    if draft.edit.is_none() {
        let text = if draft.relative {
            format!(
                "{}{}s",
                if draft.value > 0.0 { "+" } else { "" },
                number::relative(draft.value)
            )
        } else {
            number::absolute(draft.value)
        };
        ui.painter().with_clip_rect(rect).text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            style::font(20.880_001 / 1.48),
            style::TEXT,
        );
        if starting {
            let text = if draft.relative {
                number::relative(draft.value)
            } else {
                number::absolute(draft.value)
            };
            draft.edit = Some(super::ValueEdit {
                stable: text.clone(),
                text,
                composing: false,
            });
            egui::text_edit::TextEditState::default().store(ui.ctx(), id);
        }
    }
    let mut finish = false;
    if let Some(edit) = &mut draft.edit {
        let was_composing = edit.composing;
        let mut ime_frame = false;
        let mut committed = false;
        let mut disabled = false;
        if focused_before && ui.is_enabled() {
            ui.input(|input| {
                for event in &input.events {
                    if let egui::Event::Ime(event) = event {
                        ime_frame = true;
                        match event {
                            egui::ImeEvent::Preedit { text, .. } => {
                                if !text.is_empty() && !edit.composing {
                                    edit.stable.clone_from(&edit.text);
                                }
                                edit.composing = !text.is_empty();
                            }
                            egui::ImeEvent::Commit(_) => {
                                edit.composing = false;
                                committed = true;
                            }
                            #[expect(deprecated)]
                            egui::ImeEvent::Disabled => {
                                edit.composing = false;
                                disabled = true;
                            }
                            _ => {}
                        }
                    }
                }
            });
        }
        let composition_guard = was_composing || edit.composing || ime_frame;
        let escape = focused_before
            && ui.is_enabled()
            && !composition_guard
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if escape {
            draft.edit = None;
            egui::text_edit::TextEditState::default().store(ui.ctx(), id);
            ui.memory_mut(|m| m.surrender_focus(id));
            paint::field_outline(ui, rect, false, hovered);
            return;
        }
        let response = ui.put(
            rect.shrink2(Vec2::new(12.0, 4.0)),
            egui::TextEdit::singleline(&mut edit.text)
                .id(id)
                .char_limit(usize::MAX)
                .horizontal_align(egui::Align::Center)
                .vertical_align(egui::Align::Center)
                .font(style::font(20.880_001 / 1.48))
                .text_color(style::TEXT)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO),
        );
        if starting {
            response.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(edit.text.chars().count()),
                    )));
                state.store(ui.ctx(), id);
            }
        }
        if committed {
            edit.stable.clone_from(&edit.text);
        }
        let has_focus = window_focused && (response.has_focus() || starting);
        if disabled || (!has_focus && edit.composing) {
            edit.text.clone_from(&edit.stable);
            edit.composing = false;
            egui::text_edit::TextEditState::default().store(ui.ctx(), id);
        }
        if !composition_guard && !edit.composing {
            let normalized = normalized(&edit.text);
            if edit.text != normalized {
                edit.text = normalized;
            }
            edit.stable.clone_from(&edit.text);
            finish = response.lost_focus() || !has_focus;
        }
        if finish {
            draft.value = number::parse(&edit.text, draft.value, !draft.relative);
        }
    }
    if finish {
        draft.edit = None;
        egui::text_edit::TextEditState::default().store(ui.ctx(), id);
        ui.memory_mut(|m| m.surrender_focus(id));
    }
    paint::field_outline(
        ui,
        rect,
        draft.edit.is_some() && ui.memory(|m| m.has_focus(id)),
        hovered,
    );
}
