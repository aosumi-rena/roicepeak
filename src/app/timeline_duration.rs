#[path = "timeline_number.rs"]
mod number;
#[path = "timeline_duration_paint.rs"]
mod paint;
use super::mora::{Mora, Word};
use crate::app::{
    document::{
        Block,
        state::{DurationMode, DurationState},
    },
    style,
};
use eframe::egui::{self, Color32, Id, Rect, Sense, Stroke, Vec2};

#[derive(Clone, Default)]
pub(super) struct Editor {
    target: Option<Id>,
    text: String,
    composition: super::composition::Composition,
    field_id: Option<egui::Id>,
    address: Option<(u64, u64, usize, usize)>,
    mode: Option<DurationMode>,
    rect: Option<Rect>,
}
impl Editor {
    pub(super) fn load(ctx: &egui::Context) -> Self {
        ctx.data_mut(|d| d.get_temp(Id::new("duration-editor")))
            .unwrap_or_default()
    }
    pub(super) fn store(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Id::new("duration-editor"), self));
    }
    pub(super) fn mora(
        &mut self,
        ui: &mut egui::Ui,
        block: &mut Block,
        word: &Word,
        mora: &Mora,
        card: Rect,
        cell: Rect,
        id: Id,
        color: Color32,
        word_hovered: bool,
        enabled: bool,
    ) {
        let enabled = enabled && ui.is_enabled();
        if !enabled {
            self.target = None;
            self.field_id = None;
            self.composition = super::composition::Composition::default();
        }
        let Some(mut duration) = mora.duration else {
            return;
        };
        let center = cell.center().x;
        let drag_enabled = enabled && crate::app::phoneme_history::allowed(ui.ctx());
        let history_target = crate::app::phoneme_history::Target {
            block: block.id,
            sentence: word.sentence_id,
            lane: crate::app::phoneme_history::Lane::Duration,
        };
        let mut dragging = false;
        if duration.mode == DurationMode::Relative {
            let width = (28.0 * duration.value) as f32;
            let bar = Rect::from_center_size(
                egui::pos2(center, card.top() + 87.0),
                Vec2::new(width, 47.0),
            );
            let hit = bar.expand2(Vec2::new(2.0, 0.0));
            let response = ui.interact(
                hit,
                id.with("bar"),
                if drag_enabled {
                    Sense::click_and_drag()
                } else {
                    Sense::hover()
                },
            );
            let origin_id = id.with("bar-origin");
            let mut capture =
                crate::app::phoneme_history::load_capture::<(f32, f32)>(ui.ctx(), origin_id);
            let held =
                response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down());
            let released = ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary));
            let mut started = crate::app::phoneme_history::capture_started_this_frame::<(f32, f32)>(
                ui.ctx(),
                origin_id,
            );
            if drag_enabled
                && capture.is_none()
                && crate::app::phoneme_history::pressed(ui.ctx())
                && (held || response.clicked())
            {
                let origin = ui
                    .input(|i| i.pointer.press_origin())
                    .or_else(|| response.interact_pointer_pos())
                    .unwrap_or(bar.center());
                let side = if origin.x < center { -1.0 } else { 1.0 };
                capture = Some((side, origin.x - (center + side * (width * 0.5 + 2.0))));
                crate::app::phoneme_history::store_capture(ui.ctx(), origin_id, capture.unwrap());
                started = true;
            }
            if let Some((side, offset)) = capture {
                if drag_enabled {
                    crate::app::phoneme_history::note(
                        ui.ctx(),
                        origin_id,
                        history_target,
                        released || !ui.input(|i| i.pointer.primary_down()),
                    );
                }
                if drag_enabled
                    && (released
                        || (!started && held && ui.input(|i| i.pointer.delta() != Vec2::ZERO)))
                {
                    if let Some(point) = ui.input(|i| i.pointer.interact_pos()) {
                        duration.value =
                            (((point.x - offset - center) * side / 14.0) as f64).clamp(0.5, 2.0);
                        mora.duration_value(block, duration.value);
                    }
                }
                dragging = drag_enabled && ui.input(|i| i.pointer.primary_down());
                if released || !drag_enabled || !ui.input(|i| i.pointer.primary_down()) {
                    crate::app::phoneme_history::clear_capture::<(f32, f32)>(ui.ctx(), origin_id);
                } else {
                    crate::app::phoneme_history::store_capture(ui.ctx(), origin_id, (side, offset));
                }
            }
            if response.hovered()
                && ui
                    .input(|i| i.pointer.hover_pos())
                    .is_some_and(|p| p.x <= hit.left() + 8.0 || p.x >= hit.right() - 8.0)
            {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
        let absolute = duration.mode == DurationMode::Absolute;
        let rect = Rect::from_center_size(
            egui::pos2(center, card.top() + if absolute { 87.0 } else { 124.5 }),
            Vec2::new(if absolute { 48.0 } else { 56.0 }, 20.0),
        );
        let visible = absolute || word_hovered || dragging || self.target == Some(id);
        if !visible {
            paint::bar(ui, card, cell, duration, color, false, false);
            return;
        }
        let number = if absolute {
            format!("{:.0}", (duration.value * 1000.0).round())
        } else {
            let value = ((duration.value - 1.0) * 100.0).round();
            if value == 0.0 {
                "±0".into()
            } else {
                format!("{value:+.0}")
            }
        };
        let suffix = if absolute { "ms" } else { "%" };
        let font = style::font(11.0);
        let number_width = ui
            .painter()
            .layout_no_wrap(number.clone(), font.clone(), style::TEXT)
            .size()
            .x;
        let suffix_width = ui
            .painter()
            .layout_no_wrap(suffix.into(), font.clone(), style::TEXT)
            .size()
            .x;
        let suffix_x = center + (number_width - suffix_width) * 0.5;
        let numeric = if absolute {
            Rect::from_min_max(rect.min, egui::pos2(suffix_x, rect.bottom()))
        } else {
            rect
        };
        let suffix_rect = Rect::from_min_max(egui::pos2(suffix_x, rect.top()), rect.max);
        let mut initial = false;
        if self.target != Some(id) {
            let response = ui.interact(
                numeric,
                id.with("number"),
                if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            if response.clicked() {
                self.target = Some(id);
                self.address = Some((block.id, word.sentence_id, word.token, mora.index));
                self.mode = Some(duration.mode);
                self.rect = Some(rect);
                self.text = if absolute {
                    format!("{:.0}", duration.value * 1000.0)
                } else {
                    format!("{:.0}", (duration.value - 1.0) * 100.0)
                };
                self.composition = super::composition::Composition::default();
                initial = true;
            }
            if absolute {
                let response = ui.interact(
                    suffix_rect,
                    id.with("suffix"),
                    if drag_enabled {
                        Sense::click_and_drag()
                    } else {
                        Sense::hover()
                    },
                );
                let origin = id.with("vertical-origin");
                let shift = ui.input(|i| i.modifiers.shift);
                let held =
                    response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down());
                let released =
                    ui.input(|i| i.pointer.button_released(egui::PointerButton::Primary));
                let mut capture =
                    crate::app::phoneme_history::load_capture::<(f64, f32, bool)>(ui.ctx(), origin);
                let mut started = crate::app::phoneme_history::capture_started_this_frame::<(
                    f64,
                    f32,
                    bool,
                )>(ui.ctx(), origin);
                if drag_enabled
                    && capture.is_none()
                    && crate::app::phoneme_history::pressed(ui.ctx())
                    && (held || response.clicked())
                {
                    let point = ui
                        .input(|i| i.pointer.press_origin())
                        .or_else(|| response.interact_pointer_pos())
                        .unwrap_or(suffix_rect.center());
                    capture = Some((duration.value, point.y, false));
                    crate::app::phoneme_history::store_capture(ui.ctx(), origin, capture.unwrap());
                    started = true;
                }
                if let Some(mut initial_value) = capture {
                    if drag_enabled {
                        crate::app::phoneme_history::note(
                            ui.ctx(),
                            origin,
                            history_target,
                            released || !ui.input(|i| i.pointer.primary_down()),
                        );
                    }
                    if drag_enabled
                        && (released
                            || (!started
                                && held
                                && (ui.input(|i| i.pointer.delta() != Vec2::ZERO)
                                    || initial_value.2 != shift)))
                    {
                        if let Some(point) = ui.input(|i| i.pointer.interact_pos()) {
                            if initial_value.2 != shift {
                                initial_value = (duration.value, point.y, shift);
                                crate::app::phoneme_history::store_capture(
                                    ui.ctx(),
                                    origin,
                                    initial_value,
                                );
                            }
                            let dy = point.y as f64 - initial_value.1 as f64;
                            let value = initial_value.0 - dy / if shift { 1000.0 } else { 100.0 };
                            duration.value = (value.clamp(0.01, 10.0) * 200.0).round() / 200.0;
                            mora.duration_value(block, duration.value);
                        }
                    }
                    if released || !drag_enabled || !ui.input(|i| i.pointer.primary_down()) {
                        crate::app::phoneme_history::clear_capture::<(f64, f32, bool)>(
                            ui.ctx(),
                            origin,
                        );
                    } else {
                        crate::app::phoneme_history::store_capture(ui.ctx(), origin, initial_value);
                    }
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
            }
        }
        paint::bar(
            ui,
            card,
            cell,
            duration,
            color,
            word_hovered,
            self.target == Some(id) || dragging,
        );
        if self.target != Some(id) {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{number}{suffix}"),
                font,
                if word_hovered { color } else { style::TEXT },
            );
            return;
        }
        let field_id = id.with("input");
        self.field_id = Some(field_id);
        let focused = initial || ui.memory(|m| m.has_focus(field_id));
        let (enter, escape) = self.composition.keys(ui, focused);
        ui.painter().rect_filled(rect, 0, Color32::WHITE);
        ui.painter().rect_stroke(
            rect,
            0,
            Stroke::new(1.0, style::BORDER),
            egui::StrokeKind::Inside,
        );
        let response = {
            let mut buffer = number::Buffer(&mut self.text);
            ui.put(
                rect,
                egui::TextEdit::singleline(&mut buffer)
                    .id(field_id)
                    .font(style::font(11.0))
                    .horizontal_align(egui::Align::Center)
                    .vertical_align(egui::Align::Center)
                    .margin(egui::Margin::ZERO)
                    .frame(egui::Frame::NONE),
            )
        };
        self.rect = Some(rect);
        if initial {
            response.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field_id) {
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(self.text.chars().count()),
                    )));
                state.store(ui.ctx(), field_id);
            }
        }
        if escape
            || self
                .composition
                .finish(enter || (!initial && response.lost_focus()))
        {
            if !escape && enabled {
                if let Some(value) = self
                    .mode
                    .and_then(|mode| number::duration(&self.text, mode))
                {
                    mora.duration(block, value);
                }
            }
            self.target = None;
            self.field_id = None;
            self.composition = super::composition::Composition::default();
            if response.has_focus() {
                ui.memory_mut(|m| m.surrender_focus(field_id));
            }
        }
    }
}

pub(super) fn finish_pointer_edit(
    ctx: &egui::Context,
    project: &mut crate::app::document::Project,
    selected: Option<u64>,
    visible: bool,
    enabled: bool,
    ui_enabled: bool,
) {
    let Some(mut editor) = ctx.data_mut(|d| d.get_temp::<Editor>(Id::new("duration-editor")))
    else {
        return;
    };
    if editor.target.is_none() {
        return;
    }
    if !ui_enabled {
        editor.target = None;
        editor.field_id = None;
        editor.composition = super::composition::Composition::default();
        editor.store(ctx);
        return;
    }
    editor.composition.refresh(ctx, editor.field_id, true);
    let Some((block_id, sentence_id, token, mora)) = editor.address else {
        return;
    };
    let outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| editor.rect.is_some_and(|r| !r.contains(p)))
    });
    if !editor
        .composition
        .finish(outside || selected != Some(block_id) || !visible)
    {
        editor.store(ctx);
        return;
    }
    if enabled {
        if let Some(value) = editor
            .mode
            .and_then(|mode| number::duration(&editor.text, mode))
        {
            if let Some(block) = project.blocks.iter_mut().find(|b| b.id == block_id) {
                if let Some(sentence_index) =
                    block.sentences.iter().position(|s| s.id == sentence_id)
                {
                    let sentence = &mut block.sentences[sentence_index];
                    if let Some(word) = sentence.words.get_mut(token) {
                        let language = word.language;
                        if let Some(mora) = word.morae.get_mut(mora) {
                            mora.set_duration(language, value);
                        }
                    }
                    for segment in block
                        .segments
                        .iter_mut()
                        .filter(|s| s.sentence.index == sentence_index)
                    {
                        crate::app::document::state::project(
                            &sentence.words,
                            std::slice::from_mut(segment),
                        );
                    }
                }
            }
        }
    }
    editor.target = None;
    editor.field_id = None;
    editor.composition = super::composition::Composition::default();
    editor.store(ctx);
}

pub(super) fn special(block: &Block, word: &Word) -> bool {
    block
        .sentences
        .get(word.sentence_index)
        .and_then(|s| s.words.get(word.token))
        .is_some_and(|w| {
            w.morae.len() == 1
                && w.morae[0].phones.len() == 1
                && matches!(w.morae[0].phones[0].symbol.as_str(), "sil" | "pau")
        })
}
pub(super) fn mode(
    ui: &mut egui::Ui,
    block: &mut Block,
    word: &Word,
    card: Rect,
    enabled: bool,
    color: Color32,
) {
    if !special(block, word) {
        return;
    }
    let Some(mora) = word.morae.first() else {
        return;
    };
    let Some(current) = block
        .sentences
        .get(word.sentence_index)
        .and_then(|s| s.words.get(word.token))
        .and_then(|w| w.morae.first())
        .and_then(|m| m.first_duration(word.language))
    else {
        return;
    };
    let rect = Rect::from_center_size(
        egui::pos2(card.center().x, card.top() + 44.0),
        Vec2::new(48.0, 16.0),
    );
    for (index, (mode, text)) in [
        (DurationMode::Relative, "%"),
        (DurationMode::Absolute, "ms"),
    ]
    .into_iter()
    .enumerate()
    {
        let half = Rect::from_min_size(
            rect.min + Vec2::new(index as f32 * 24.0, 0.0),
            Vec2::new(24.0, 16.0),
        );
        let response = ui.interact(
            half,
            ui.id().with((
                "duration-mode",
                block.id,
                word.sentence_id,
                word.token,
                index,
            )),
            if enabled && mora.raw_query_duration_seconds.is_some() {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        ui.painter().text(
            half.center(),
            egui::Align2::CENTER_CENTER,
            text,
            style::font(11.0),
            if current.mode == mode {
                color
            } else {
                style::TEXT
            },
        );
        if response.clicked() {
            if let Some(converted) = mora
                .raw_query_duration_seconds
                .and_then(|raw| current.converted(mode, raw))
            {
                mora.duration(
                    block,
                    DurationState {
                        mode,
                        value: converted.value,
                    },
                );
            }
        }
    }
}
