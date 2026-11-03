#[path = "native_alert.rs"]
pub(crate) mod native_alert;
use crate::app::{
    document::Project,
    editor::{EditorApp, PendingAction, Popup},
};
use eframe::egui::{self, Rect, Vec2};

impl EditorApp {
    pub(crate) fn isolate_alert_focus(&self, ctx: &egui::Context) {
        if self.error.is_some() {
            native_alert::begin(ctx, egui::Id::new("editor-notice"));
        } else if self.pending.is_some() {
            native_alert::begin(ctx, egui::Id::new("unsaved-project"));
        } else {
            crate::app::emotion_presets::isolate_focus(ctx);
        }
    }

    pub fn request_action(&mut self, action: PendingAction, ctx: &egui::Context) {
        if self.dirty {
            self.pending = Some(action);
        } else {
            self.execute_action(action, ctx);
        }
    }
    fn execute_action(&mut self, action: PendingAction, ctx: &egui::Context) {
        if matches!(action, PendingAction::ImportSrt | PendingAction::ImportSsml) {
            self.pending = None;
            let imported = match action {
                PendingAction::ImportSrt => self.import_srt(),
                PendingAction::ImportSsml => self.import_ssml(),
                _ => unreachable!(),
            };
            if let Err(error) = imported {
                self.error = Some(error);
            }
            return;
        }
        self.pending = None;
        match action {
            PendingAction::New => {
                self.invalidate_audio();
                self.history_navigation = true;
                self.query_basis_current = false;
                self.project = Project::default();
                self.project.export_settings =
                    self.resources.export_settings.clone().unwrap_or_default();
                self.portrait_state.clear_choices();
                self.project.blocks[0].narrator = self.default_narrator();
                self.filename = None;
                self.select_only(0);
                self.dirty = false;
                self.undo.clear();
                self.redo.clear();
                self.scroll = 0.0;
            }
            PendingAction::Open => self.open(),
            PendingAction::OpenPath(path) => self.open_path(path),
            PendingAction::ImportSrt | PendingAction::ImportSsml => unreachable!(),
            PendingAction::Quit => {
                self.invalidate_audio();
                self.history_navigation = true;
                self.quit_approved = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
    pub fn save(&mut self, save_as: bool) {
        let path = if !save_as {
            self.filename.clone()
        } else {
            None
        }
        .or_else(|| {
            rfd::FileDialog::new()
                .add_filter(self.tr("", "ROICEPEAK project"), &["roicepeak"])
                .add_filter("VOICEPEAK project", &["vpp"])
                .set_file_name(
                    if self
                        .filename
                        .as_deref()
                        .is_some_and(crate::app::project_files::native)
                    {
                        "Untitled.vpp"
                    } else {
                        "Untitled.roicepeak"
                    },
                )
                .save_file()
        });
        if let Some(path) = path {
            match crate::app::project_files::write(
                &path,
                &self.project,
                self.render_config.as_ref(),
            ) {
                Ok(()) => {
                    if let Err(error) = self.recent_projects.record(&path) {
                        self.error = Some(error);
                    }
                    self.filename = Some(path);
                    self.dirty = false;
                }
                Err(e) => self.error = Some(e),
            }
        }
    }
    pub fn open(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(self.tr("", "ROICEPEAK project"), &["roicepeak", "json"])
            .add_filter("VOICEPEAK project", &["vpp"])
            .pick_file()
        {
            self.open_path(path);
        }
    }
    pub(crate) fn open_path(&mut self, path: std::path::PathBuf) {
        match crate::app::project_files::read(&path, self.render_config.as_ref()) {
            Ok(project) => {
                self.query_basis_current = false;
                self.invalidate_audio();
                self.history_navigation = true;
                self.project = project;
                self.portrait_state.clear_choices();
                if let Err(error) = self.recent_projects.record(&path) {
                    self.error = Some(error);
                }
                self.filename = Some(path);
                self.selection.clear();
                self.dirty = false;
                self.undo.clear();
                self.redo.clear();
                self.prepare_project();
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub(crate) fn import(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(self.tr("", "UTF-8 text"), &["txt"])
            .pick_file()
        {
            let result = (|| -> Result<_, String> {
                use std::io::Read;
                let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
                let mut bytes = Vec::new();
                file.take(16 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 16 * 1024 * 1024 {
                    return Err("Text import exceeds 16 MiB".into());
                }
                let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
                let room = 10_000usize.saturating_sub(self.project.blocks.len());
                if text
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .take(room + 1)
                    .count()
                    > room
                {
                    return Err("Text import exceeds the project block limit".into());
                }
                let mut project = self.project.clone();
                let narrator = self.append_narrator(None);
                let first = project.blocks.len();
                project.import_text(&text);
                for block in &mut project.blocks[first..] {
                    block.narrator = narrator.clone();
                }
                project.validate()?;
                Ok(project)
            })();
            match result {
                Ok(project) => self.project = project,
                Err(e) => self.error = Some(e.to_string()),
            }
        }
    }
    pub fn popups(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, screen: Rect) {
        self.popup_content(ui, ctx, screen);
        self.emotion_preset_actions(ui);
        crate::app::timestamp_popup::show(self, ui, ctx, screen);
    }
    fn popup_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, screen: Rect) {
        if self.popup != Some(Popup::EmotionPreset) {
            crate::app::native_menu::close(ctx, egui::Id::new("emotion-preset-menu"));
        }
        if self.popup != Some(Popup::File) {
            crate::app::file_menu::close(ctx);
        }
        let Some(popup) = self.popup else {
            return;
        };
        if popup == Popup::File {
            self.file_menu(ui, ctx, screen);
            return;
        }
        if popup == Popup::Narrator {
            self.narrator_picker(ctx, screen);
            return;
        }
        if popup == Popup::EmotionPreset {
            self.emotion_preset_popup(ui);
            return;
        }
        let toolbar = crate::app::toolbar::origin(screen.width());
        let position = match popup {
            Popup::File => screen.min + Vec2::new(toolbar, 39.0),
            Popup::Speed => screen.min + Vec2::new(toolbar + 46.0, 36.0),
            Popup::Pitch => screen.min + Vec2::new(toolbar + 90.0, 36.0),
            Popup::Pause => screen.min + Vec2::new(toolbar + 134.0, 36.0),
            Popup::Volume => screen.min + Vec2::new(toolbar + 178.0, 36.0),
            Popup::Emotion => {
                screen.min
                    + Vec2::new(
                        (toolbar + 354.0 - 157.0).clamp(0.0, (screen.width() - 314.0).max(0.0)),
                        45.0,
                    )
            }
            Popup::Narrator => {
                self.inspector_rect(Rect::from_min_max(
                    screen.min + Vec2::new(0.0, 44.0),
                    screen.max,
                ))
                .min + Vec2::new(16.0, 76.0)
            }
            Popup::EmotionPreset => unreachable!("Emotion presets use native menu geometry"),
        };
        egui::Area::new(egui::Id::new("editor-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .show(ctx, |ui| {
                let scalar_popup = matches!(
                    popup,
                    Popup::Speed | Popup::Pitch | Popup::Pause | Popup::Volume
                );
                if popup == Popup::Emotion {
                    let background = ui.painter().add(egui::Shape::Noop);
                    let shown =
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::same(8))
                            .show(ui, |ui| {
                                self.narrator_settings_popup(
                                    ui,
                                    screen.bottom() - position.y - 16.0,
                                );
                            });
                    crate::app::anchored_popup::paint(ui, background, shown.response.rect);
                    return;
                }
                let background = scalar_popup.then(|| ui.painter().add(egui::Shape::Noop));
                let frame = if scalar_popup {
                    egui::Frame::NONE
                } else {
                    egui::Frame::popup(ui.style())
                };
                let shown = frame.show(ui, |ui| match popup {
                    Popup::File => unreachable!("File menus use the shared menu renderer"),
                    Popup::Speed | Popup::Pitch | Popup::Pause | Popup::Volume => {
                        let (_, outer) = ui.allocate_space(Vec2::new(264.0, 69.0));
                        let (name, min, max, value) = match popup {
                            Popup::Speed => (
                                crate::app::i18n::tr(self.locale, "速さ", "Speed"),
                                50.0,
                                200.0,
                                &mut self.project.global.speed,
                            ),
                            Popup::Pitch => (
                                crate::app::i18n::tr(self.locale, "ピッチ", "Pitch"),
                                -150.0,
                                150.0,
                                &mut self.project.global.pitch,
                            ),
                            Popup::Volume => (
                                crate::app::i18n::tr(self.locale, "音量", "Volume"),
                                0.0,
                                200.0,
                                &mut self.project.global.volume,
                            ),
                            _ => (
                                crate::app::i18n::tr(self.locale, "ポーズ", "Pause"),
                                50.0,
                                200.0,
                                &mut self.project.global.pause,
                            ),
                        };
                        let mut display = value.percent();
                        let response =
                            crate::app::style::nonnegative(ui, popup == Popup::Volume, |ui| {
                                crate::app::style::parameter_row(
                                    ui,
                                    ("global-slider", popup as u8),
                                    Rect::from_min_size(
                                        outer.min + Vec2::new(28.0, 21.0),
                                        Vec2::new(208.0, 26.0),
                                    ),
                                    &mut display,
                                    min,
                                    max,
                                    if popup == Popup::Pitch { 0.0 } else { 100.0 },
                                    true,
                                    name,
                                    egui::Color32::from_rgb(95, 95, 102),
                                )
                            });
                        if response.changed() {
                            *value = crate::app::document::ControlValue::from_percent(display);
                        }
                    }
                    Popup::Emotion => unreachable!("Narrator corrections use an anchored host"),
                    Popup::Narrator => {
                        unreachable!("Narrator selectors have their own native layouts")
                    }
                    Popup::EmotionPreset => {
                        unreachable!("Emotion presets use native menu geometry")
                    }
                });
                if let Some(background) = background {
                    crate::app::anchored_popup::paint(ui, background, shown.response.rect);
                }
            });
    }
    pub fn dialogs(&mut self, ctx: &egui::Context) {
        if self.error.is_none() && self.pending.is_none() {
            self.emotion_preset_dialogs(ctx);
        }
        if let Some(action) = self.pending.clone() {
            let title = self.tr("未保存の変更", "Unsaved Changes").to_owned();
            let message = self.tr("変更を保存しますか？", "Save changes?").to_owned();
            let labels = [
                self.tr("はい", "Yes"),
                self.tr("いいえ", "No"),
                self.tr("キャンセル", "Cancel"),
            ];
            match native_alert::show(
                ctx,
                egui::Id::new("unsaved-project"),
                &title,
                &message,
                native_alert::Kind::Information,
                &labels,
            ) {
                Some(0) => {
                    self.save(false);
                    if !self.dirty {
                        self.execute_action(action, ctx);
                    }
                }
                Some(1) => self.execute_action(action, ctx),
                Some(_) => self.pending = None,
                None => (),
            }
        }
        if self.preferences {
            let title = self.tr("環境設定...", "Preferences...").to_owned();
            let open =
                crate::app::dialogs::viewport(ctx, "preferences", &title, [480.0, 480.0], |ui| {
                    self.preferences_content(ui)
                });
            self.preferences &= open;
        }
        if self.dictionary {
            self.begin_dictionary_window();
            let title = self
                .tr("ユーザー辞書の管理...", "Manage User Dictionary...")
                .to_owned();
            self.dictionary =
                crate::app::dialogs::viewport(ctx, "dictionary", &title, [672.0, 440.0], |ui| {
                    self.dictionary_content(ui)
                });
            if self.dictionary && self.dictionary_ui.draft.is_some() {
                let title = self
                    .tr("新しい単語を登録...", "Register word...")
                    .to_owned();
                let mut keep = true;
                let open = crate::app::dialogs::viewport(
                    ctx,
                    "dictionary-draft",
                    &title,
                    [432.0, 392.0],
                    |ui| {
                        keep = self.dictionary_draft(ui);
                    },
                );
                if !open || !keep {
                    self.dictionary_ui.draft = None;
                }
            }
            if !self.dictionary {
                self.end_dictionary_window();
            }
        }
        if self.about {
            self.about_window(ctx);
        }
        if let Some(message) = self.error.clone() {
            let title = self.tr("お知らせ", "Notice").to_owned();
            let message = crate::app::i18n::message(self.locale, &message);
            if native_alert::show(
                ctx,
                egui::Id::new("editor-notice"),
                &title,
                &message,
                native_alert::Kind::Error,
                &[self.tr("", "OK")],
            )
            .is_some()
            {
                self.error = None;
            }
        }
    }
}
