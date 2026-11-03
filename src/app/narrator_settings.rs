use crate::app::{
    catalog::EmotionControl,
    editor::EditorApp,
    style::{self, Icon},
};
use eframe::egui::{self, Color32, Rect, Sense, Vec2};

struct Row {
    identity: String,
    instance: String,
    name: String,
    controls: Vec<EmotionControl>,
    expanded: bool,
    avatar: bool,
}

impl Row {
    fn id(&self) -> egui::Id {
        egui::Id::new(("narrator-correction-row", &self.identity))
    }

    fn height(&self) -> f32 {
        if self.expanded {
            204.0 + self.controls.len() as f32 * 35.0
        } else {
            52.0
        }
    }
}

impl EditorApp {
    pub(crate) fn narrator_settings_popup(&mut self, ui: &mut egui::Ui, max_height: f32) {
        let rows: Vec<_> = crate::app::narrator_order::identity_indices(&self.catalog)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|index| {
                let voice = &self.catalog.voices[index];
                let metadata = self.catalog.identity_metadata(&voice.name)?;
                let primary = self.catalog.identity_source(&voice.name)?;
                let controls = metadata
                    .preset_controls
                    .iter()
                    .map(|binding| EmotionControl {
                        binding: binding.clone(),
                        enabled: metadata
                            .emotions
                            .iter()
                            .any(|emotion| emotion.key == binding.key && emotion.enabled),
                    })
                    .collect();
                let row = Row {
                    identity: voice.name.clone(),
                    instance: primary.key.clone(),
                    name: crate::app::i18n::narrator_name(self.locale, &voice.name).to_owned(),
                    controls,
                    expanded: self.resources.expanded_corrections.contains(&voice.name),
                    avatar: self.avatar_cache.contains_key(&primary.key),
                };
                Some(row)
            })
            .collect();
        let content_height = 48.0 + rows.iter().map(Row::height).sum::<f32>();
        let size = Vec2::new(298.0, content_height.min(max_height.max(0.0)));
        let (_, viewport) = ui.allocate_space(size);
        crate::app::inspector_scroll::show_content(
            ui,
            viewport,
            Vec2::new(298.0, content_height),
            Vec2::new(3.0, 10.0),
            false,
            |ui, content| {
                let title = Rect::from_min_size(
                    content.min + Vec2::new(20.0, 15.0),
                    Vec2::new(258.0, 25.0),
                );
                ui.painter()
                    .with_clip_rect(title.intersect(ui.clip_rect()))
                    .text(
                        title.left_center(),
                        egui::Align2::LEFT_CENTER,
                        crate::app::i18n::text(self.locale, "Narrator Voice Settings"),
                        style::font(20.880_001 * 1000.0 / 1480.0),
                        Color32::from_gray(102),
                    );
                let mut y = content.top() + 48.0;
                let mut boundaries = Vec::with_capacity(rows.len());
                for row in rows {
                    let rect = Rect::from_min_size(
                        egui::pos2(content.left(), y),
                        Vec2::new(298.0, row.height()),
                    );
                    self.narrator_correction_row(ui, rect, &row);
                    y += row.height();
                    boundaries.push(y);
                }
                for bottom in boundaries {
                    ui.painter().rect_filled(
                        Rect::from_min_size(
                            egui::pos2(content.left(), bottom),
                            Vec2::new(298.0, 1.0),
                        ),
                        0,
                        Color32::from_black_alpha(31),
                    );
                }
            },
        );
    }

    fn narrator_correction_row(&mut self, ui: &mut egui::Ui, rect: Rect, row: &Row) {
        let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 52.0));
        let response = ui.interact(header, row.id().with("header"), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(header, 0, Color32::from_black_alpha(13));
        }
        let color = self.instance_color(&row.instance);
        if row.avatar {
            self.paint_instance_avatar(
                ui,
                &row.instance,
                Rect::from_min_size(rect.min + Vec2::new(20.0, 10.0), Vec2::splat(32.0)),
            );
        }
        let left = if row.avatar { 64.0 } else { 20.0 };
        let label = Rect::from_min_size(
            rect.min + Vec2::new(left, 10.0),
            Vec2::new(rect.width() - left - 20.0, 32.0),
        );
        ui.painter()
            .with_clip_rect(label.intersect(ui.clip_rect()))
            .text(
                label.left_center(),
                egui::Align2::LEFT_CENTER,
                &row.name,
                style::font(20.880_001 * 1000.0 / 1480.0),
                color,
            );
        let mut more_clicked = false;
        if row.expanded {
            let more = Rect::from_min_size(
                rect.min + Vec2::new(rect.width() - 52.0, 10.0),
                Vec2::splat(32.0),
            );
            more_clicked = style::icon(
                ui,
                row.id().with("more"),
                more,
                Icon::PresetMore,
                style::TEXT,
                crate::app::i18n::text(self.locale, "Preset settings"),
            )
            .clicked();
            if more_clicked {
                crate::app::emotion_presets::open_correction_actions(
                    self,
                    ui.ctx(),
                    &row.identity,
                    more,
                );
            }
        } else {
            let center = rect.min + Vec2::new(rect.width() - 28.0, 26.0);
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    center + Vec2::new(-6.0, -3.0),
                    center + Vec2::new(6.0, -3.0),
                    center + Vec2::new(0.0, 3.0),
                ],
                Color32::from_gray(179),
                egui::Stroke::NONE,
            ));
        }
        if response.clicked() && !more_clicked {
            if row.expanded {
                self.resources.expanded_corrections.remove(&row.identity);
            } else {
                self.resources
                    .expanded_corrections
                    .insert(row.identity.clone());
            }
            if let Some(draft) = &mut self.resource_draft {
                draft.expanded_corrections = self.resources.expanded_corrections.clone();
            }
            if let Err(error) = self.resources.save() {
                self.error = Some(error);
            }
            ui.ctx().request_repaint();
        }
        if !row.expanded {
            return;
        }
        let mut parameters = self
            .project
            .narrator_settings
            .get(&row.identity)
            .cloned()
            .unwrap_or_default();
        let before = parameters.clone();
        let locale = self.locale;
        for (index, (key, value, min, max, default)) in [
            ("Speed", &mut parameters.speed, 50.0, 200.0, 100.0),
            ("Pitch", &mut parameters.pitch, -150.0, 150.0, 0.0),
            ("Pause", &mut parameters.pause, 50.0, 200.0, 100.0),
            ("Volume", &mut parameters.volume, 0.0, 200.0, 100.0),
        ]
        .into_iter()
        .enumerate()
        {
            let inner = Rect::from_min_size(
                rect.min + Vec2::new(20.0, 56.0 + index as f32 * 35.0),
                Vec2::new(258.0, 26.0),
            );
            let mut display = value.percent();
            let response = style::nonnegative(ui, index == 3, |ui| {
                style::parameter_row_padded(
                    ui,
                    (row.id(), "scalar", index),
                    inner,
                    &mut display,
                    min,
                    max,
                    default,
                    true,
                    crate::app::i18n::text(locale, key),
                    color,
                    28.0,
                )
            });
            if response.changed() {
                *value = crate::app::document::ControlValue::from_percent(display);
            }
        }
        for (index, control) in row.controls.iter().enumerate() {
            let inner = Rect::from_min_size(
                rect.min + Vec2::new(20.0, 198.0 + index as f32 * 35.0),
                Vec2::new(258.0, 26.0),
            );
            let value = parameters
                .emotions
                .entry(control.binding.key.clone())
                .or_default();
            let old = *value;
            let mut display = value.percent();
            let response = style::parameter_row_padded(
                ui,
                (row.id(), "emotion", &control.binding.key),
                inner,
                &mut display,
                -100.0,
                100.0,
                0.0,
                control.enabled,
                crate::app::i18n::text(locale, &control.binding.label),
                color,
                28.0,
            );
            if response.changed() {
                *value = crate::app::document::ControlValue::from_percent(display);
            }
            if *value != old {
                self.portrait_state
                    .note_narrator_change(&row.identity, &control.binding.key);
            }
        }
        let changed = parameters.speed != before.speed
            || parameters.pitch != before.pitch
            || parameters.pause != before.pause
            || parameters.volume != before.volume
            || parameters.emotions.iter().any(|(key, value)| {
                *value != before.emotions.get(key).copied().unwrap_or_default()
            });
        if changed {
            self.project
                .narrator_settings
                .insert(row.identity.clone(), parameters);
        }
    }
}
