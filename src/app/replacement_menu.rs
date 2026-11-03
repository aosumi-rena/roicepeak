use super::{Choice, EditorApp};
use crate::app::narrator_picker::menu;
use eframe::egui;

pub(super) fn show(
    app: &mut EditorApp,
    parent: &egui::Ui,
    anchor: &egui::Response,
    index: usize,
    row: &mut Choice,
    choices: &[String],
) {
    let id = parent.id().with(("replacement-menu", index));
    if !anchor.clicked() && !egui::Popup::is_id_open(parent.ctx(), id) {
        return;
    }
    let reset = !egui::Popup::is_id_open(parent.ctx(), id) || anchor.clicked();
    let outcome = menu::show_popup(
        app,
        parent,
        egui::Popup::menu(anchor).id(id),
        id,
        reset,
        choices,
        menu::Options {
            original: Some(&row.original),
            excluded_identity: Some(&row.identity),
            minimum_width: anchor.rect.width(),
            single_column: false,
        },
    );
    if let Some(key) = outcome.key {
        row.target = key;
    }
}
