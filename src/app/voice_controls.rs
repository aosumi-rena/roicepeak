//! Voice-owned emotion labels and UI order, independent of stylebook row order.
use crate::app::{
    editor::EditorApp,
    render::EmotionBinding,
    style::{self, Icon},
};
use eframe::egui::{self, Rect, Vec2};

impl EditorApp {
    pub(crate) fn narrator_emotion_controls(
        &self,
        narrator: &crate::app::document::VoiceRef,
    ) -> Vec<EmotionBinding> {
        self.catalog
            .identity_metadata(&narrator.identity)
            .map(|metadata| {
                metadata
                    .controls
                    .iter()
                    .map(|control| control.binding.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn narrator_preset_controls(
        &self,
        narrator: &crate::app::document::VoiceRef,
    ) -> Vec<EmotionBinding> {
        self.catalog
            .identity_metadata(&narrator.identity)
            .map(|metadata| metadata.preset_controls.clone())
            .unwrap_or_default()
    }

    pub(crate) fn narrator_emotion_enabled(
        &self,
        narrator: &crate::app::document::VoiceRef,
        key: &str,
    ) -> bool {
        self.catalog
            .identity_metadata(&narrator.identity)
            .and_then(|metadata| {
                metadata
                    .controls
                    .iter()
                    .find(|control| control.binding.key == key)
            })
            .is_some_and(|control| control.enabled)
    }

    pub(crate) fn selected_emotion_controls(&self) -> Vec<EmotionBinding> {
        let Some(block) = self
            .selected()
            .and_then(|index| self.project.blocks.get(index))
        else {
            return Vec::new();
        };
        if self
            .selected_indices()
            .iter()
            .any(|&index| self.project.blocks[index].narrator.identity != block.narrator.identity)
        {
            return Vec::new();
        }
        self.narrator_emotion_controls(&block.narrator)
    }

    pub(crate) fn emotion_preset_popup(&mut self, ui: &egui::Ui) {
        let Some(anchor) = ui
            .ctx()
            .data(|data| data.get_temp::<Rect>(egui::Id::new("emotion-preset-anchor")))
        else {
            self.popup = None;
            return;
        };
        self.emotion_preset_choices(ui, anchor);
    }

    pub(crate) fn emotion_inspector(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let controls = self.selected_emotion_controls();
        let color = self
            .selected()
            .and_then(|index| self.project.blocks.get(index))
            .map(|block| self.narrator_color(&block.narrator))
            .unwrap_or(style::TEXT);
        style::label(
            ui,
            rect.min + Vec2::new(16.0, 399.0),
            self.tr("感情", "Emotion"),
            14.0,
            style::TEXT,
        );
        let preset = Rect::from_min_size(rect.min + Vec2::new(16.0, 419.0), Vec2::new(228.0, 32.0));
        crate::app::emotion_presets::selector(self, ui, preset, color);
        let more = Rect::from_min_size(rect.min + Vec2::new(220.0, 387.0), Vec2::splat(24.0));
        if style::icon(
            ui,
            "emotion-preset-more",
            more,
            Icon::PresetMore,
            style::TEXT,
            self.tr("", "Preset settings"),
        )
        .clicked()
        {
            crate::app::emotion_presets::open_actions(self, ui.ctx(), more);
        }
        let selected = self.selected_indices();
        let selected_ids = self.selected_ids();
        for (control_index, control) in controls.iter().enumerate() {
            let row = Rect::from_min_size(
                rect.min + Vec2::new(0.0, 457.0 + control_index as f32 * 39.0),
                Vec2::new(260.0, 39.0),
            );
            let read = |index: usize| {
                self.project.blocks[index]
                    .parameters
                    .emotions
                    .get(&control.key)
                    .copied()
                    .unwrap_or_default()
            };
            let current = selected
                .first()
                .map(|&index| read(index))
                .unwrap_or_default();
            let mut value = current.percent();
            let mixed = selected
                .iter()
                .any(|&index| !read(index).same_number(current));
            let response = style::parameter_row_mixed(
                ui,
                ("emotion", &selected_ids, &control.key),
                Rect::from_min_size(row.min + Vec2::new(16.0, 6.0), Vec2::new(228.0, 27.0)),
                &mut value,
                0.0,
                100.0,
                0.0,
                self.selected()
                    .and_then(|index| self.project.blocks.get(index))
                    .is_some_and(|block| {
                        self.narrator_emotion_enabled(&block.narrator, &control.key)
                    }),
                crate::app::i18n::text(self.locale, &control.label),
                color,
                mixed,
                crate::app::i18n::text(self.locale, "Multiple Values"),
            );
            let authored = crate::app::document::ControlValue::from_percent(value);
            if response.changed() && selected.iter().any(|&index| read(index) != authored) {
                self.begin_edit();
                for &index in &selected {
                    let block = &mut self.project.blocks[index];
                    if block
                        .parameters
                        .emotions
                        .get(&control.key)
                        .copied()
                        .unwrap_or_default()
                        != authored
                    {
                        block
                            .parameters
                            .emotions
                            .insert(control.key.clone(), authored);
                        self.portrait_state.note_block_change(
                            block.id,
                            &block.narrator.identity,
                            &control.key,
                        );
                    }
                }
            }
        }
    }
}
