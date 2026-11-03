use crate::app::{editor::EditorApp, style};
use eframe::egui::{self, Color32, Rect, Stroke, StrokeKind, Vec2};

impl EditorApp {
    pub(crate) fn about_window(&mut self, ctx: &egui::Context) {
        let title = self.tr("バージョン情報", "About").to_owned();
        let body = self
            .tr(
                "Rust + egui で開発した音声合成エンジンとエディター。",
                "A speech synthesis engine and editor developed with Rust + egui.",
            )
            .to_owned();
        self.about = crate::app::dialogs::viewport(ctx, "about", &title, [350.0, 310.0], |ui| {
            let origin = ui.max_rect().min;
            let at = |x, y, w, h| Rect::from_min_size(origin + Vec2::new(x, y), Vec2::new(w, h));
            ui.painter()
                .rect_filled(at(0.0, 0.0, 350.0, 310.0), 0, style::BAR);
            egui::Image::from_bytes(
                "bytes://roicepeak/about-logo.svg",
                include_bytes!("../../assets/branding/logo.svg").as_slice(),
            )
            .tint(Color32::WHITE)
            .paint_at(ui, at(30.0, 17.0, 38.0, 30.0));
            egui::Image::from_bytes(
                "bytes://roicepeak/about-wordmark.svg",
                include_bytes!("../../assets/branding/wordmark.svg").as_slice(),
            )
            .paint_at(ui, at(80.0, 20.0, 138.7654, 27.0));
            ui.painter().rect_filled(
                at(0.0, 64.0, 350.0, 246.0),
                egui::CornerRadius {
                    nw: 12,
                    ne: 12,
                    sw: 0,
                    se: 0,
                },
                Color32::from_gray(248),
            );
            let info = at(30.0, 80.0, 290.0, 104.0);
            ui.painter().rect_filled(info, 3, Color32::WHITE);
            ui.painter()
                .rect_stroke(info, 3, Stroke::new(1.0, style::BORDER), StrokeKind::Inside);
            ui.scope_builder(egui::UiBuilder::new().max_rect(info.shrink(8.0)), |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&body)
                                .font(style::font(13.0))
                                .color(style::TEXT),
                        )
                        .wrap(),
                    );
                });
            });
        });
    }
}
