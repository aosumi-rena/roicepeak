#[path = "inspector_version.rs"]
mod version;
use crate::app::{
    editor::{EditorApp, Popup},
    style::{self, Icon},
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

impl EditorApp {
    pub(crate) fn inspector_rect(&self, body: Rect) -> Rect {
        Rect::from_min_size(
            Pos2::new(
                body.right()
                    - if self.resources.show_portrait {
                        564.0
                    } else {
                        284.0
                    },
                body.top() + 24.0,
            ),
            Vec2::new(
                260.0,
                466.0 + self.selected_emotion_controls().len() as f32 * 39.0,
            ),
        )
    }

    pub fn inspector(&mut self, ui: &mut egui::Ui, body: Rect) {
        let card = self.inspector_rect(body);
        let height = (body.height()
            - if self.mode.is_some() {
                crate::app::timeline::HEIGHT.min(body.height())
            } else {
                0.0
            })
        .max(0.0);
        if height == 0.0 {
            return;
        }
        let viewport = Rect::from_min_size(
            Pos2::new(card.left() - 24.0, body.top()),
            Vec2::new(308.0, height),
        );
        crate::app::inspector_scroll::show(ui, viewport, card.size(), |ui, rect| {
            self.inspector_content(ui, rect)
        });
    }

    fn inspector_content(&mut self, ui: &mut egui::Ui, rect: Rect) {
        style::card(ui, rect);
        let voice = Rect::from_min_size(rect.min + Vec2::splat(16.0), Vec2::new(228.0, 60.0));
        let current = self.selected();
        let key = current
            .and_then(|i| self.project.blocks.get(i))
            .map(|b| b.narrator.clone())
            .unwrap_or_default();
        let selected = self.selected_indices();
        let mixed_identity = selected.first().is_some_and(|&first| {
            selected.iter().any(|&index| {
                self.project.blocks[index].narrator.identity
                    != self.project.blocks[first].narrator.identity
            })
        });
        let voice_enabled = current.is_some() || mixed_identity;
        if current.is_some() && !mixed_identity {
            self.paint_avatar(ui, &key, Rect::from_min_size(voice.min, Vec2::splat(60.0)));
        }
        style::label(
            ui,
            voice.min + Vec2::new(68.0, 30.0),
            if mixed_identity {
                crate::app::i18n::text(self.locale, "Multiple Voices").to_owned()
            } else if current.is_none() {
                String::new()
            } else {
                self.narrator_name(&key)
            },
            16.0,
            if mixed_identity {
                style::TEXT
            } else {
                self.narrator_color(&key)
            },
        );
        style::draw_icon(
            ui,
            Pos2::new(voice.right() - 9.0, voice.center().y),
            Icon::Chevron,
            style::TEXT,
        );
        if ui
            .interact(
                voice,
                ui.id().with("inspector-voice"),
                if voice_enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            )
            .clicked()
        {
            crate::app::narrator_picker::remember_anchor(
                ui.ctx(),
                voice,
                crate::app::narrator_picker::PickerKind::Inspector,
            );
            self.toggle(Popup::Narrator);
        }
        let divider = |ui: &egui::Ui, y| {
            ui.painter().line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                Stroke::new(1.0, Color32::from_gray(228)),
            );
        };
        divider(ui, rect.top() + 92.0);
        style::label(
            ui,
            rect.min + Vec2::new(16.0, 112.0),
            self.tr("ボイス", "Voice"),
            14.0,
            style::TEXT,
        );
        style::label(
            ui,
            rect.min + Vec2::new(16.0, 147.5),
            self.tr("バージョン", "Version"),
            14.0,
            style::TEXT,
        );
        let version_rect =
            Rect::from_min_size(rect.min + Vec2::new(96.0, 132.0), Vec2::new(148.0, 31.0));
        let block_id = self
            .selected()
            .and_then(|index| self.project.blocks.get(index))
            .map(|block| block.id);
        let uniform_version = selected
            .first()
            .map(|&index| self.project.blocks[index].narrator.version)
            .filter(|version| {
                selected
                    .iter()
                    .all(|&index| self.project.blocks[index].narrator.version == *version)
            });
        if let Some(version) = version::show(
            ui,
            version_rect,
            &key,
            &self.catalog,
            self.locale,
            block_id,
            self.narrator_color(&key),
            current.is_some() && !selected.is_empty() && !mixed_identity,
            uniform_version,
        ) {
            let changed: Vec<_> = selected
                .iter()
                .copied()
                .filter(|&index| self.project.blocks[index].narrator.version != version)
                .collect();
            if !changed.is_empty() {
                self.begin_edit();
                for index in changed {
                    let block = &mut self.project.blocks[index];
                    block.narrator.version = version;
                    block.segments.clear();
                }
            }
        }
        divider(ui, rect.top() + 175.0);
        style::label(
            ui,
            rect.min + Vec2::new(16.0, 199.0),
            self.tr("設定", "Settings"),
            14.0,
            style::TEXT,
        );
        let locale = self.locale;
        let color = if mixed_identity {
            style::TEXT
        } else {
            self.narrator_color(&key)
        };
        let selected_ids = self.selected_ids();
        for (field, (label, min, max, default)) in [
            (
                crate::app::i18n::tr(locale, "速さ", "Speed"),
                50.0,
                200.0,
                100.0,
            ),
            (
                crate::app::i18n::tr(locale, "ピッチ", "Pitch"),
                -150.0,
                150.0,
                0.0,
            ),
            (
                crate::app::i18n::tr(locale, "ポーズ", "Pause"),
                50.0,
                200.0,
                100.0,
            ),
            (
                crate::app::i18n::tr(locale, "音量", "Volume"),
                0.0,
                200.0,
                100.0,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let read = |parameters: &crate::app::document::Parameters| match field {
                0 => parameters.speed,
                1 => parameters.pitch,
                2 => parameters.pause,
                _ => parameters.volume,
            };
            let current = selected
                .first()
                .map(|&index| read(&self.project.blocks[index].parameters))
                .unwrap_or(crate::app::document::ControlValue::from_percent(default));
            let mut value = current.percent();
            let mixed = selected
                .iter()
                .any(|&index| !read(&self.project.blocks[index].parameters).same_number(current));
            let response = style::nonnegative(ui, field == 3, |ui| {
                style::parameter_row_mixed(
                    ui,
                    ("setting", &selected_ids, field),
                    Rect::from_min_size(
                        Pos2::new(rect.left() + 16.0, rect.top() + 220.0 + field as f32 * 39.0),
                        Vec2::new(228.0, 27.0),
                    ),
                    &mut value,
                    min,
                    max,
                    default,
                    !selected.is_empty(),
                    label,
                    color,
                    mixed,
                    crate::app::i18n::text(locale, "Multiple Values"),
                )
            });
            let authored = crate::app::document::ControlValue::from_percent(value);
            if response.changed()
                && selected
                    .iter()
                    .any(|&index| read(&self.project.blocks[index].parameters) != authored)
            {
                self.begin_edit();
                for &index in &selected {
                    let parameters = &mut self.project.blocks[index].parameters;
                    match field {
                        0 => parameters.speed = authored,
                        1 => parameters.pitch = authored,
                        2 => parameters.pause = authored,
                        _ => parameters.volume = authored,
                    }
                }
            }
        }
        divider(ui, rect.top() + 379.0);
        self.emotion_inspector(ui, rect);
    }
}
