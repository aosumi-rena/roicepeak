use crate::app::{
    editor::{EditorApp, PendingAction},
    project_files,
};
use eframe::egui;

impl EditorApp {
    pub(crate) fn project_drop(&mut self, ctx: &egui::Context) {
        let files = ctx.input(|input| input.raw.dropped_files.clone());
        if files.is_empty() {
            return;
        }
        let path = match files.as_slice() {
            [file] if project_files::native(file.path()) => Some(file.path()),
            _ => None,
        };
        let Some(path) = path else {
            self.error = Some("Drop one VOICEPEAK project (*.vpp)".into());
            return;
        };
        self.popup = None;
        self.request_action(PendingAction::OpenPath(path.to_owned()), ctx);
    }
}
