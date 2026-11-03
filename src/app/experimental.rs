use crate::app::{editor::EditorApp, style};
use eframe::egui;

impl EditorApp {
    pub(crate) fn experimental_window(&mut self, ctx: &egui::Context) {
        if !self.experimental_settings {
            return;
        }
        let title = self
            .tr("実験的なオプション", "Experimental options")
            .to_owned();
        let mut extended = self.resources.allow_out_of_range;
        let mut negative = self.resources.allow_negative;
        let open = crate::app::dialogs::viewport(
            ctx,
            "experimental-options",
            &title,
            [480.0, 170.0],
            |ui| {
                egui::Frame::NONE.inner_margin(20.0).show(ui, |ui| {
                    style::label(
                        ui,
                        ui.cursor().min + egui::vec2(0.0, 12.0),
                        &title,
                        17.0,
                        egui::Color32::from_rgb(71, 78, 132),
                    );
                    ui.add_space(40.0);
                    ui.checkbox(&mut extended, self.tr("範囲を拡張", "Expand range"));
                    ui.add_space(12.0);
                    ui.checkbox(
                        &mut negative,
                        self.tr("負の値を許可", "Allow negative values"),
                    );
                });
            },
        );
        self.experimental_settings = open;
        if extended != self.resources.allow_out_of_range
            || negative != self.resources.allow_negative
        {
            let mut resources = self.resources.clone();
            resources.allow_out_of_range = extended;
            resources.allow_negative = negative;
            match resources.save() {
                Ok(()) => {
                    self.query_basis_current = false;
                    self.resources = resources;
                    self.invalidate_audio();
                    if let Some(config) = &mut self.render_config {
                        config.value_policy = self.resources.value_policy();
                    }
                    self.prepare_project();
                }
                Err(error) => self.error = Some(error),
            }
        }
    }
}
