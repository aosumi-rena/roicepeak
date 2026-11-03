//! Editor child windows.
#[path = "preferences.rs"]
mod preferences;
use eframe::egui::{self, Color32, Vec2};

pub fn viewport(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    size: [f32; 2],
    mut content: impl FnMut(&mut egui::Ui),
) -> bool {
    let center = ctx.input(|i| i.viewport().inner_rect.map(|r| r.center()));
    let mut builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size(size)
        .with_resizable(false)
        .with_minimize_button(false)
        .with_maximize_button(false);
    if let Some(center) = center {
        builder = builder.with_position(center - Vec2::new(size[0] / 2.0, size[1] / 2.0));
    }
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(id), builder, |ui, _| {
        let open = !ui.input(|i| i.viewport().close_requested());
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::WHITE))
            .show(ui, |ui| content(ui));
        open
    })
}
