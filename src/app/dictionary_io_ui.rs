use super::{EditorApp, file};
use crate::app::{menus::native_alert, native_icons, native_menu, style};
use eframe::egui::{self, Rect};

#[derive(Clone, Copy)]
enum Action {
    Import,
    Export,
}

impl EditorApp {
    pub(super) fn dictionary_more(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let dictionary = self.dictionary_state.display_snapshot();
        let id = ui.id().with("dictionary-more-menu");
        let response = ui.interact(rect, ui.id().with("dictionary-more"), egui::Sense::click());
        let title = self.tr("辞書", "Dictionary");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
        style::button_paint(ui, rect, &response, false, true);
        native_icons::paint(ui, rect, style::Icon::PresetMore, false, response.hovered());
        let clicked = response.clicked();
        if clicked && egui::Popup::is_id_open(ui.ctx(), id) {
            native_menu::close(ui.ctx(), id);
            return;
        }
        if clicked || egui::Popup::is_id_open(ui.ctx(), id) {
            let items = [
                native_menu::Item::Action {
                    text: self.tr("辞書のインポート", "Import Dictionary"),
                    shortcut: String::new(),
                    enabled: self.dictionary_writable(),
                    action: Action::Import,
                },
                native_menu::Item::Action {
                    text: self.tr("辞書のエクスポート", "Export Dictionary"),
                    shortcut: String::new(),
                    enabled: !dictionary.entries().is_empty(),
                    action: Action::Export,
                },
            ];
            match native_menu::show_at_width(ui, id, rect, &items, 212.0).action {
                Some(Action::Import) => {
                    self.dictionary_ui.import_path = rfd::FileDialog::new()
                        .set_title(self.tr("辞書のインポート", "Import Dictionary"))
                        .add_filter("VOICEPEAK dictionary", &["vdc", "vdc2"])
                        .pick_file();
                }
                Some(Action::Export) => {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title(self.tr("辞書のエクスポート", "Export Dictionary"))
                        .add_filter("VOICEPEAK dictionary", &["vdc2"])
                        .save_file()
                    {
                        if let Err(error) = file::write(&path, dictionary.entries()) {
                            self.dictionary_ui.file_error = Some(error);
                        }
                    }
                }
                None => {}
            }
        }
        if self.dictionary_ui.import_path.is_some() {
            let labels = [
                self.tr("マージ", "Merge"),
                self.tr("置換", "Replace"),
                self.tr("キャンセル", "Cancel"),
            ];
            if let Some(choice) = native_alert::show(
                ui.ctx(),
                ui.id().with("dictionary-import-mode"),
                self.tr("辞書のインポート", "Import Dictionary"),
                self.tr(
                    "現在の辞書にマージしますか、置換しますか？",
                    "Merge or replace the current dictionary?",
                ),
                native_alert::Kind::Information,
                &labels,
            ) {
                let path = self.dictionary_ui.import_path.take().unwrap();
                if choice < 2 {
                    let entries = file::read(&path)
                        .and_then(|incoming| {
                            if choice == 0 {
                                file::merge(dictionary.entries(), &incoming)
                            } else {
                                Ok(incoming)
                            }
                        })
                        .and_then(|entries| {
                            self.commit_dictionary(&dictionary, entries)?;
                            Ok(())
                        });
                    match entries {
                        Ok(()) => {
                            self.dictionary_ui.selected = None;
                            self.dictionary_ui.draft = None;
                        }
                        Err(error) => self.dictionary_ui.file_error = Some(error),
                    }
                }
            }
        }
        if let Some(error) = self.dictionary_ui.file_error.clone() {
            if native_alert::show(
                ui.ctx(),
                ui.id().with("dictionary-file-error"),
                self.tr("辞書", "Dictionary"),
                &error,
                native_alert::Kind::Error,
                &["OK"],
            )
            .is_some()
            {
                self.dictionary_ui.file_error = None;
            }
        }
    }
}
