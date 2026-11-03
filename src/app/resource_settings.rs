//! Explicit source paths for resource detection, persisted in ROICEPEAK preferences.
use crate::app::{dialog_controls, editor::EditorApp, resources::Locations, style};
use eframe::egui::{self, Color32, Rect, Sense, Vec2};
use std::path::PathBuf;

fn row(top: &mut f32, bounds: Rect, height: f32) -> Rect {
    let rect = Rect::from_min_size(
        egui::pos2(bounds.left(), *top),
        Vec2::new(bounds.width(), height),
    );
    *top += height;
    rect
}

fn label(ui: &egui::Ui, rect: Rect, text: &str, heading: bool) {
    ui.interact(
        rect,
        ui.id().with(("resource-label", rect.top().to_bits())),
        Sense::hover(),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
    ui.painter()
        .with_clip_rect(rect.intersect(ui.clip_rect()))
        .text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            text,
            style::font(if heading { 17.0 } else { 14.0 }),
            if heading {
                dialog_controls::HEADING
            } else {
                style::TEXT
            },
        );
}

fn list(ui: &mut egui::Ui, rect: Rect, salt: &str, content: impl FnOnce(&mut egui::Ui)) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = Vec2::new(8.0, 4.0);
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .max_height(rect.height())
        .min_scrolled_height(0.0)
        .min_scrolled_width(0.0)
        .auto_shrink([false, false])
        .show(&mut child, |ui| {
            // Keep the floating scrollbar clear of path actions and wrapped text.
            ui.set_max_width((ui.available_width() - 16.0).max(0.0));
            content(ui);
        });
}

impl EditorApp {
    pub(crate) fn open_resource_settings(&mut self) {
        self.resource_draft = Some(self.resources.clone());
        self.resource_settings = true;
    }

    pub(crate) fn resource_window(&mut self, ctx: &egui::Context) {
        if !self.resource_settings {
            return;
        }
        let mut draft = self
            .resource_draft
            .take()
            .unwrap_or_else(|| self.resources.clone());
        let title = self
            .tr("リソースの検索場所", "Resource search paths")
            .to_owned();
        let mut close = false;
        let open =
            crate::app::dialogs::viewport(ctx, "resource-settings", &title, [720.0, 540.0], |ui| {
                let bounds = ui.max_rect().shrink(16.0);
                let enabled = ui.is_enabled();
                let remaining = (bounds.height() - 256.0).max(64.0);
                let sources_height = (remaining * 0.6).clamp(32.0, 150.0);
                let voices_height = (remaining - sources_height).clamp(32.0, 112.0);
                let mut top = bounds.top();
                label(
                    ui,
                    row(&mut top, bounds, 24.0),
                    self.tr("ボイスバンクの検索場所", "Voicebank search paths"),
                    true,
                );
                top += 8.0;
                let mut remove = None;
                list(
                    ui,
                    row(&mut top, bounds, sources_height),
                    "voice-source-paths",
                    |ui| {
                        for (index, path) in draft.voice_roots.iter_mut().enumerate() {
                            ui.push_id(("resource-source-row", index), |ui| {
                                let (path_row, _) = ui.allocate_exact_size(
                                    Vec2::new(ui.available_width(), 32.0),
                                    Sense::hover(),
                                );
                                let button = Rect::from_min_size(
                                    egui::pos2(path_row.right() - 96.0, path_row.top()),
                                    Vec2::new(96.0, 32.0),
                                );
                                let field = Rect::from_min_max(
                                    path_row.min + Vec2::new(0.0, 2.0),
                                    egui::pos2(
                                        (button.left() - 8.0).max(path_row.left()),
                                        path_row.bottom() - 2.0,
                                    ),
                                );
                                let mut text = path.to_string_lossy().into_owned();
                                let id = ui.id().with(("source", index));
                                if dialog_controls::text_field_with_limit(
                                    ui,
                                    field,
                                    id,
                                    &mut text,
                                    "",
                                    false,
                                    usize::MAX,
                                )
                                .changed()
                                {
                                    *path = PathBuf::from(text);
                                }
                                if style::text_button(
                                    ui,
                                    button,
                                    self.tr("削除", "Remove"),
                                    false,
                                    enabled,
                                )
                                .clicked()
                                {
                                    remove = Some(index);
                                }
                            });
                        }
                    },
                );
                if let Some(index) = remove {
                    draft.voice_roots.remove(index);
                }
                top += 8.0;
                let buttons = row(&mut top, bounds, 32.0);
                let add = Rect::from_min_size(
                    buttons.min,
                    Vec2::new(300.0_f32.min(buttons.width() - 128.0), 32.0),
                );
                let rescan = Rect::from_min_size(
                    add.right_top() + Vec2::new(8.0, 0.0),
                    Vec2::new(120.0, 32.0),
                );
                if style::text_button(
                    ui,
                    add,
                    self.tr("フォルダーを追加...", "Add folder..."),
                    false,
                    enabled && draft.voice_roots.len() < 32,
                )
                .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        if !draft.voice_roots.contains(&path) {
                            draft.voice_roots.push(path);
                        }
                    }
                }
                if style::text_button(ui, rescan, self.tr("再検索", "Rescan"), false, enabled)
                    .clicked()
                {
                    if let Err(error) = self.apply_resource_locations(draft.clone()) {
                        self.error = Some(error);
                    }
                }
                top += 12.0;
                dialog_controls::section(
                    ui,
                    row(&mut top, bounds, 20.0),
                    self.tr("発音辞書の場所", "Pronunciation dictionary path"),
                );
                top += 6.0;
                let dictionary = row(&mut top, bounds, 32.0);
                let browse = Rect::from_min_size(
                    egui::pos2(dictionary.right() - 96.0, dictionary.top()),
                    Vec2::new(96.0, 32.0),
                );
                let dictionary_field = Rect::from_min_max(
                    dictionary.min + Vec2::new(0.0, 2.0),
                    egui::pos2(
                        (browse.left() - 8.0).max(dictionary.left()),
                        dictionary.bottom() - 2.0,
                    ),
                );
                let mut text = draft
                    .dictionary
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let dictionary_id = ui.id().with("dictionary-path");
                if dialog_controls::text_field_with_limit(
                    ui,
                    dictionary_field,
                    dictionary_id,
                    &mut text,
                    "",
                    false,
                    usize::MAX,
                )
                .changed()
                {
                    draft.dictionary = (!text.trim().is_empty()).then(|| PathBuf::from(text));
                }
                if style::text_button(ui, browse, self.tr("参照...", "Browse..."), false, enabled)
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                        draft.dictionary = Some(path);
                    }
                }
                top += 8.0;
                let portrait = row(&mut top, bounds, 24.0);
                let caption = self.tr("立ち絵を表示", "Show portrait");
                let portrait_id = ui.id().with("show-resource-portrait");
                dialog_controls::toggle(
                    ui,
                    Rect::from_min_size(portrait.min, Vec2::new(32.0, 24.0)),
                    portrait_id,
                    &mut draft.show_portrait,
                    caption,
                );
                let portrait_label =
                    Rect::from_min_max(portrait.min + Vec2::new(44.0, 0.0), portrait.max);
                label(ui, portrait_label, caption, false);
                if ui
                    .interact(portrait_label, portrait_id.with("label"), Sense::click())
                    .clicked()
                    && enabled
                {
                    draft.show_portrait = !draft.show_portrait;
                }
                top += 12.0;
                label(
                    ui,
                    row(&mut top, bounds, 20.0),
                    &format!(
                        "{} {}",
                        self.catalog.voices.len(),
                        self.tr("ボイスを検出", "voices detected")
                    ),
                    false,
                );
                top += 6.0;
                list(
                    ui,
                    row(&mut top, bounds, voices_height),
                    "detected-voices",
                    |ui| {
                        for voice in &self.catalog.voices {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "{}  v{}  {}",
                                        crate::app::i18n::narrator_name(self.locale, &voice.name),
                                        voice.version,
                                        voice.vendor
                                    ))
                                    .font(style::font(14.0))
                                    .color(style::TEXT),
                                )
                                .wrap(),
                            );
                        }
                        for problem in &self.catalog.problems {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(problem)
                                        .font(style::font(14.0))
                                        .color(Color32::DARK_RED),
                                )
                                .wrap(),
                            );
                        }
                    },
                );
                top += 12.0;
                let actions = row(&mut top, bounds, 32.0);
                let close_button = Rect::from_min_size(
                    egui::pos2(actions.right() - 120.0, actions.top()),
                    Vec2::new(120.0, 32.0),
                );
                let apply = close_button.translate(Vec2::new(-128.0, 0.0));
                if style::text_button(ui, apply, self.tr("適用", "Apply"), true, enabled).clicked()
                {
                    if let Err(error) = self.apply_resource_locations(draft.clone()) {
                        self.error = Some(error);
                    }
                }
                if style::text_button(ui, close_button, self.tr("閉じる", "Close"), false, enabled)
                    .clicked()
                {
                    close = true;
                }
            });
        self.resource_settings = open && !close;
        self.resource_draft = self.resource_settings.then_some(draft);
    }

    fn apply_resource_locations(&mut self, draft: Locations) -> Result<(), String> {
        let mut locations = self.resources.clone();
        locations.voice_roots = draft.voice_roots;
        locations.dictionary = draft.dictionary;
        locations.show_portrait = draft.show_portrait;
        locations.validate()?;
        let catalog = crate::app::catalog::Catalog::load_roots(&locations.voice_roots)?;
        let config = locations
            .dictionary
            .clone()
            .filter(|_| !catalog.voices.is_empty())
            .map(|dictionary| crate::app::render::Config::from_catalog(dictionary, &catalog))
            .transpose()?;
        locations.save()?;
        self.resources = locations;
        self.replace_voice_catalog(catalog, config);
        Ok(())
    }
}
