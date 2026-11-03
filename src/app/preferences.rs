use crate::app::{dialog_controls, editor::EditorApp, style};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};

fn control(origin: Pos2, top: f32, height: f32) -> Rect {
    Rect::from_min_size(origin + Vec2::new(225.0, top), Vec2::new(230.0, height))
}

fn font_caption(value: f32) -> String {
    format!("{value} pt")
}

fn font_step(ui: &mut egui::Ui, rect: Rect, id: egui::Id, sign: &str, enabled: bool) -> bool {
    let enabled = enabled && ui.is_enabled();
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, sign));
    if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
        ui.painter()
            .rect_filled(rect, 3, Color32::from_black_alpha(13));
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        sign,
        style::font(20.0),
        if enabled {
            style::TEXT
        } else {
            Color32::from_gray(179)
        },
    );
    response.clicked() && enabled
}

impl EditorApp {
    pub fn preferences_content(&mut self, ui: &mut egui::Ui) {
        let origin = ui.max_rect().min;
        for (top, ja, en) in [
            (16.0, "ユーザーインターフェース", "User interface"),
            (178.0, "音声設定", "Audio settings"),
            (346.0, "ボイス設定", "Voice Settings"),
        ] {
            style::label(
                ui,
                origin + Vec2::new(20.0, top + 16.0),
                self.tr(ja, en),
                17.0,
                dialog_controls::HEADING,
            );
        }
        for (center, ja, en) in [
            (70.0, "言語", "Language"),
            (110.0, "フォントサイズ", "Font size"),
            (234.0, "オーディオシステム", "Audio system"),
            (278.0, "出力機器", "Audio device"),
            (322.0, "サンプリング周波数", "Sample rate"),
            (402.0, "既定のボイス", "Default Voice"),
        ] {
            style::label(
                ui,
                origin + Vec2::new(20.0, center),
                self.tr(ja, en),
                14.0,
                style::TEXT,
            );
        }
        let languages = crate::app::i18n::Locale::ALL
            .into_iter()
            .map(|locale| (locale, locale.name()))
            .collect::<Vec<_>>();
        let language_id = ui.id().with("language");
        if dialog_controls::combo(
            ui,
            control(origin, 56.0, 24.0),
            language_id,
            &mut self.locale,
            &languages,
        ) {
            self.set_locale(self.locale);
        }
        self.preferences_font_control(ui, origin);
        let left = Rect::from_min_size(origin + Vec2::new(20.0, 138.0), Vec2::new(215.0, 31.0));
        let right = left.translate(Vec2::new(224.0, 0.0));
        if style::text_button(
            ui,
            left,
            self.tr("リソースの検索場所", "Resource search paths"),
            false,
            true,
        )
        .clicked()
        {
            self.preferences = false;
            self.open_resource_settings();
        }
        if style::text_button(
            ui,
            right,
            self.tr("実験的なオプション...", "Experimental options..."),
            false,
            true,
        )
        .clicked()
        {
            self.preferences = false;
            self.experimental_settings = true;
        }
        let system = if cfg!(windows) {
            "Windows Audio"
        } else {
            "System audio"
        };
        let system_id = ui.id().with("audio-system");
        dialog_controls::combo(
            ui,
            control(origin, 218.0, 32.0),
            system_id,
            &mut 0u8,
            &[(0u8, system)],
        );
        let default_name = self
            .tr("システムの既定出力", "System default output")
            .to_owned();
        let output_rect = control(origin, 262.0, 32.0);
        let output_id = ui.id().with("audio-output");
        let changed = {
            let choices = std::iter::once((String::new(), default_name.as_str()))
                .chain(
                    self.audio_outputs
                        .iter()
                        .map(|output| (output.id.clone(), output.name.as_str())),
                )
                .collect::<Vec<_>>();
            dialog_controls::combo(ui, output_rect, output_id, &mut self.output_id, &choices)
        };
        if !self.output_id.is_empty()
            && !self
                .audio_outputs
                .iter()
                .any(|output| output.id == self.output_id)
        {
            let text_rect = Rect::from_min_max(
                output_rect.min + Vec2::new(6.0, 0.0),
                output_rect.max - Vec2::new(24.0, 0.0),
            );
            ui.painter().with_clip_rect(text_rect).text(
                text_rect.left_center(),
                egui::Align2::LEFT_CENTER,
                default_name,
                style::font(14.0),
                style::TEXT,
            );
        }
        if changed {
            if let Err(error) = self.rebind_output() {
                self.error = Some(error);
            }
        }
        let rate_id = ui.id().with("render-sample-rate");
        dialog_controls::combo(
            ui,
            control(origin, 306.0, 32.0),
            rate_id,
            &mut 48000u32,
            &[(48000u32, "48000 Hz")],
        );
        self.default_voice_control(ui, control(origin, 386.0, 32.0));
    }

    fn preferences_font_control(&mut self, ui: &mut egui::Ui, origin: Pos2) {
        let id = ui.id().with("preferences-font-size");
        let draft_id = id.with("draft");
        let mut draft = ui
            .data_mut(|data| data.get_temp::<String>(draft_id))
            .unwrap_or_else(|| font_caption(self.font_points));
        if !ui.memory(|memory| memory.has_focus(id) || memory.had_focus_last_frame(id)) {
            draft = font_caption(self.font_points);
        }
        let response = dialog_controls::text_field(
            ui,
            Rect::from_min_size(origin + Vec2::new(225.0, 96.0), Vec2::new(162.0, 28.0)),
            id,
            &mut draft,
            "",
            true,
        );
        let cancel = response.has_focus()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let enter = response.has_focus()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        if cancel {
            draft = font_caption(self.font_points);
            response.surrender_focus();
        } else if response.lost_focus() || enter {
            let text = draft.trim();
            if let Ok(value) = text
                .strip_suffix("pt")
                .unwrap_or(text)
                .trim()
                .parse::<f32>()
            {
                if value.is_finite() {
                    self.font_points = value.clamp(8.0, 72.0);
                }
            }
            draft = font_caption(self.font_points);
            if enter {
                response.surrender_focus();
            }
        }
        for (x, sign, label) in [(407.0, -1.0, "−"), (438.0, 1.0, "+")] {
            let enabled = if sign < 0.0 {
                self.font_points > 8.0
            } else {
                self.font_points < 72.0
            };
            if font_step(
                ui,
                Rect::from_center_size(origin + Vec2::new(x, 110.0), Vec2::splat(28.0)),
                id.with(label),
                label,
                enabled,
            ) {
                self.font_points = (self.font_points + sign).clamp(8.0, 72.0);
                draft = font_caption(self.font_points);
                response.surrender_focus();
            }
        }
        ui.data_mut(|data| data.insert_temp(draft_id, draft));
    }

    fn default_voice_control(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let key = self.default_narrator();
        let id = ui.id().with("preferences-default-voice");
        let response = ui.interact(rect, id.with("button"), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::ComboBox,
                !self.catalog.voices.is_empty(),
                self.narrator_name(&key),
            )
        });
        ui.painter()
            .rect_filled(rect, 4, Color32::from_black_alpha(31));
        ui.painter()
            .rect_filled(rect.shrink(1.0), 4, Color32::WHITE);
        if response.hovered() || response.is_pointer_button_down_on() {
            ui.painter()
                .rect_filled(rect.shrink(1.0), 4, Color32::from_black_alpha(13));
        }
        self.paint_avatar(
            ui,
            &key,
            Rect::from_min_size(rect.min + Vec2::new(7.0, 3.0), Vec2::splat(25.0)),
        );
        let caption = Rect::from_min_size(rect.min + Vec2::new(43.0, 6.0), Vec2::new(155.0, 20.0));
        ui.painter().with_clip_rect(caption).text(
            caption.left_center(),
            egui::Align2::LEFT_CENTER,
            self.narrator_name(&key),
            style::font(23.0 / 1.48),
            self.narrator_color(&key),
        );
        let points: Vec<_> = [
            (1.0, 0.0),
            (6.0, 5.0),
            (11.0, 0.0),
            (12.0, 1.0),
            (6.5, 7.0),
            (5.5, 7.0),
            (0.0, 1.0),
        ]
        .into_iter()
        .map(|(x, y)| rect.min + Vec2::new(207.0 + x, 14.0 + y))
        .collect();
        let mut mesh = egui::Mesh::default();
        for point in &points {
            mesh.colored_vertex(*point, Color32::from_gray(179));
        }
        for [a, b, c] in [[0, 1, 6], [1, 5, 6], [1, 4, 5], [1, 2, 3], [1, 3, 4]] {
            mesh.add_triangle(a, b, c);
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        if response.clicked() || egui::Popup::is_id_open(ui.ctx(), id) {
            let indices = match crate::app::narrator_order::identity_indices(&self.catalog) {
                Ok(indices) => indices,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            };
            let choices = indices
                .into_iter()
                .map(|index| self.catalog.voices[index].key.clone())
                .collect::<Vec<_>>();
            let outcome = crate::app::narrator_picker::menu::show_at(
                self,
                ui,
                id,
                rect,
                response.clicked(),
                &choices,
            );
            if let Some(key) = outcome.key {
                let Some(voice) = self.catalog.instance(&key) else {
                    return;
                };
                let mut resources = self.resources.clone();
                resources.preferred_voice = Some(voice.name.clone());
                match resources.save() {
                    Ok(()) => {
                        if let Some(draft) = &mut self.resource_draft {
                            draft.preferred_voice = resources.preferred_voice.clone();
                        }
                        self.resources = resources;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
    }
}
