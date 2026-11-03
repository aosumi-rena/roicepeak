#[path = "timeline_composition.rs"]
mod composition;
#[path = "timeline_curve.rs"]
mod curve;
#[path = "timeline_duration.rs"]
mod duration;
#[path = "timeline_mora.rs"]
mod mora;
#[path = "timeline_pointer.rs"]
mod pointer;
#[path = "timeline_reading.rs"]
mod reading;
#[path = "timeline_reset.rs"]
mod reset;
#[path = "timeline_scroll.rs"]
mod scroll;
use crate::app::{
    editor::{EditorApp, Mode},
    style::{self, Icon},
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::hash::{Hash, Hasher};

pub const HEIGHT: f32 = 172.0;

impl EditorApp {
    pub(crate) fn finish_reading_edit(&mut self, ctx: &egui::Context) {
        let ui_enabled = !(self.pending.is_some()
            || self.error.is_some()
            || self.about
            || self.preferences
            || self.dictionary
            || self.export.active()
            || self.resource_settings
            || self.experimental_settings
            || self.shortcut_settings
            || self.voice_replacement.is_some()
            || crate::app::emotion_presets::active(ctx));
        let selected = self
            .selected()
            .and_then(|i| self.project.blocks.get(i))
            .map(|b| b.id);
        if reading::finish_pointer_edit(
            ctx,
            &mut self.project,
            selected,
            self.query_basis_current,
            self.mode.is_some(),
            ui_enabled,
            self.locale,
            &mut self.error,
        ) {
            self.query_basis_current = false;
        }
        duration::finish_pointer_edit(
            ctx,
            &mut self.project,
            selected,
            self.mode == Some(Mode::Duration),
            self.query_basis_current,
            ui_enabled && self.error.is_none(),
        );
    }
    pub fn timeline(&mut self, ui: &mut egui::Ui, body: Rect) {
        self.finish_reading_edit(ui.ctx());
        let panel_top = body.bottom() - HEIGHT.min(body.height());
        let top = panel_top + 12.0;
        if self.mode.is_some() {
            let panel = Rect::from_min_max(Pos2::new(body.left(), panel_top), body.max);
            ui.painter()
                .rect_filled(panel, 0, Color32::from_rgb(242, 242, 242));
            ui.painter().line_segment(
                [panel.left_top(), panel.right_top()],
                Stroke::new(1.0, style::BORDER),
            );
            ui.painter().line_segment(
                [
                    Pos2::new(body.left() + 59.0, panel.top()),
                    Pos2::new(body.left() + 59.0, panel.bottom()),
                ],
                Stroke::new(1.0, style::BORDER),
            );
        }
        for (index, (mode, icon, ja, en)) in [
            (Mode::Accent, Icon::Accent, "アクセント", "Accent"),
            (
                Mode::Intonation,
                Icon::Intonation,
                "イントネーション",
                "Intonation",
            ),
            (Mode::Duration, Icon::Duration, "長さ", "Duration"),
        ]
        .into_iter()
        .enumerate()
        {
            let rect = Rect::from_min_size(
                Pos2::new(body.left() + 12.0, top + index as f32 * 44.0),
                Vec2::new(36.0, 36.0),
            );
            if style::icon_state(
                ui,
                ("mode", index),
                rect,
                icon,
                if self.mode == Some(mode) {
                    Color32::WHITE
                } else {
                    style::TEXT
                },
                self.tr(ja, en),
                self.mode == Some(mode),
            )
            .clicked()
            {
                self.mode = if self.mode == Some(mode) {
                    None
                } else {
                    Some(mode)
                };
            }
        }
        let Some(mode) = self.mode else {
            return;
        };
        let accent = self
            .selected()
            .and_then(|index| self.project.blocks.get(index))
            .map(|block| self.narrator_color(&block.narrator))
            .unwrap_or(style::TEXT);
        let Some(block) = self.selected().and_then(|i| self.project.blocks.get_mut(i)) else {
            return;
        };
        let region = Rect::from_min_max(Pos2::new(body.left() + 60.0, panel_top), body.max);
        let words = mora::words(block);
        let content_width = 18.0
            + words
                .iter()
                .map(|w| w.morae.len().max(1) as f32 * 56.0)
                .sum::<f32>();
        let lane = match mode {
            Mode::Accent => "Accent",
            Mode::Intonation => "Intonation",
            Mode::Duration => "Duration",
        };
        ui.scope_builder(egui::UiBuilder::new().max_rect(region), |ui| {
            ui.set_clip_rect(region.intersect(ui.clip_rect()));
            scroll::show(ui, region, content_width, |ui, content| {
                let mut x = content.left() + 10.0;
                let mut accent_groups: Vec<(u64, Vec<Pos2>, f32)> = Vec::new();
                let mut reading_editor = reading::Editor::load(ui.ctx(), block.id);
                let mut duration_editor = duration::Editor::load(ui.ctx());
                for word in words {
                    let width = word.morae.len().max(1) as f32 * 56.0;
                    let card = Rect::from_min_size(
                        Pos2::new(x, content.top() + 16.0),
                        Vec2::new(width, 140.0),
                    );
                    style::card(ui, card);
                    let header = Rect::from_min_size(card.min, Vec2::new(width, 27.0));
                    ui.painter().line_segment(
                        [header.left_bottom(), header.right_bottom()],
                        Stroke::new(1.0, style::BORDER),
                    );
                    ui.painter().text(
                        header.center(),
                        egui::Align2::CENTER_CENTER,
                        &word.text,
                        style::font(16.0),
                        style::TEXT,
                    );
                    let mut accent_points = Vec::new();
                    let word_hovered = ui.rect_contains_pointer(card);
                    let word_id =
                        ui.id()
                            .with(("word-plot", lane, block.id, word.sentence_id, word.token));
                    let gesture_id = word_id.with("accent-gesture");
                    let mut shape = std::collections::hash_map::DefaultHasher::new();
                    word.text.hash(&mut shape);
                    word.language.hash(&mut shape);
                    for mora in &word.morae {
                        mora.reading.hash(&mut shape);
                    }
                    let shape = shape.finish();
                    let gesture = matches!(mode, Mode::Accent | Mode::Intonation)
                        && pointer::captured(ui, gesture_id, shape);
                    let enabled = (self.query_basis_current || gesture)
                        && (mode != Mode::Accent || word.language == 1)
                        && (mode == Mode::Duration || !duration::special(block, &word));
                    let response = ui.interact(
                        card,
                        word_id,
                        if enabled && mode != Mode::Duration {
                            Sense::click_and_drag()
                        } else {
                            Sense::hover()
                        },
                    );
                    let edit_point = matches!(mode, Mode::Accent | Mode::Intonation)
                        .then(|| {
                            pointer::point(
                                ui,
                                &response,
                                gesture_id,
                                shape,
                                enabled,
                                card.top() + 60.0,
                                crate::app::phoneme_history::Target {
                                    block: block.id,
                                    sentence: word.sentence_id,
                                    lane: if mode == Mode::Accent {
                                        crate::app::phoneme_history::Lane::Accent
                                    } else {
                                        crate::app::phoneme_history::Lane::Intonation
                                    },
                                },
                            )
                        })
                        .flatten();
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Slider,
                            enabled,
                            format!("{} {lane}", word.text),
                        )
                    });
                    for (column, group) in word.morae.iter().enumerate() {
                        let cell = Rect::from_min_size(
                            card.min + Vec2::new(column as f32 * 56.0, 28.0),
                            Vec2::new(56.0, 110.0),
                        );
                        if !(mode == Mode::Duration && duration::special(block, &word)) {
                            ui.painter().text(
                                cell.center_top() + Vec2::new(0.0, 18.0),
                                egui::Align2::CENTER_CENTER,
                                &group.reading,
                                style::font(14.0),
                                style::TEXT,
                            );
                        }
                        let id = ui.id().with((
                            "mora",
                            lane,
                            block.id,
                            word.sentence_id,
                            word.token,
                            group.index,
                        ));
                        if mode == Mode::Duration {
                            duration_editor.mora(
                                ui,
                                block,
                                &word,
                                group,
                                card,
                                cell,
                                id,
                                accent,
                                word_hovered,
                                self.query_basis_current,
                            );
                            continue;
                        }
                        let active = edit_point.is_some_and(|point| {
                            point.x >= card.left()
                                && point.x <= card.right()
                                && (((point.x - card.left()) / 56.0).floor() as usize)
                                    .min(word.morae.len().saturating_sub(1))
                                    == column
                        });
                        match mode {
                            Mode::Accent => {
                                let mut high = group.high;
                                if active {
                                    if let Some(point) = edit_point {
                                        high = point.y < card.top() + 95.5;
                                        group.accent(block, high);
                                    }
                                }
                                let point = Pos2::new(
                                    cell.center().x,
                                    card.top() + if high { 64.0 } else { 127.0 },
                                );
                                accent_points.push(point);
                            }
                            Mode::Intonation => {
                                let mut value = group.intonation;
                                if active {
                                    if let Some(point) = edit_point {
                                        value = (card.top() + 127.0
                                            - point.y.clamp(card.top() + 64.0, card.top() + 127.0))
                                            / 63.0
                                            * 6.0
                                            - 3.0;
                                        group.intonation(block, value);
                                    }
                                }
                                let value = if value.is_nan() { 0.0 } else { value };
                                accent_points.push(Pos2::new(
                                    cell.center().x,
                                    card.top() + 127.0
                                        - ((value + 3.0) / 6.0).clamp(0.0, 1.0) * 63.0,
                                ));
                            }
                            Mode::Duration => unreachable!(),
                        }
                    }
                    if matches!(mode, Mode::Accent | Mode::Intonation) {
                        if let Some((_, points, _)) = accent_groups
                            .last_mut()
                            .filter(|(sentence, _, _)| *sentence == word.sentence_id)
                        {
                            points.extend(accent_points);
                        } else {
                            accent_groups.push((
                                word.sentence_id,
                                accent_points,
                                card.bottom() - 13.0,
                            ));
                        }
                    }
                    if !(mode == Mode::Duration && duration::special(block, &word)) {
                        if reading_editor.word(
                            ui,
                            block,
                            &word,
                            card,
                            self.query_basis_current,
                            self.locale,
                            &mut self.error,
                        ) {
                            self.query_basis_current = false;
                        }
                    }
                    if mode == Mode::Duration {
                        duration::mode(ui, block, &word, card, self.query_basis_current, accent);
                    }
                    if reset::word(
                        ui,
                        block,
                        &word,
                        card,
                        mode,
                        self.query_basis_current,
                        self.locale,
                    ) {
                        self.query_basis_current = false;
                    }
                    x += width;
                }
                reading_editor.store(ui.ctx());
                duration_editor.store(ui.ctx());
                for (_, points, bottom) in accent_groups {
                    curve::accent(ui, &points, bottom, accent);
                }
            });
        });
    }
}
