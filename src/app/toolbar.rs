//! Toolbar controls and layout.
use crate::app::{
    document::timestamp,
    editor::{EditorApp, Popup},
    shortcuts::Command,
    style::{self, Icon},
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};

impl EditorApp {
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 44.0));
        ui.painter().rect_filled(bar, 0, style::BAR);
        let logo = Rect::from_min_size(rect.min + Vec2::new(16.0, 10.0), Vec2::splat(24.0));
        egui::Image::from_bytes(
            "bytes://roicepeak/toolbar-logo.svg",
            include_bytes!("../../assets/branding/logo.svg").as_slice(),
        )
        .paint_at(ui, logo);
        ui.interact(logo, ui.id().with("application-logo"), Sense::hover())
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "ROICEPEAK"));
        let wordmark_size = Vec2::new(92.51027, 18.0);
        if 48.0 + wordmark_size.x <= origin(rect.width()) - 8.0 {
            let wordmark = Rect::from_min_size(rect.min + Vec2::new(48.0, 13.0), wordmark_size);
            egui::Image::from_bytes(
                "bytes://roicepeak/toolbar-wordmark.svg",
                include_bytes!("../../assets/branding/wordmark.svg").as_slice(),
            )
            .paint_at(ui, wordmark);
            ui.interact(
                wordmark,
                ui.id().with("application-wordmark"),
                Sense::hover(),
            )
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "roicepeak"));
        }
        let base = rect.min;
        let button = |ui: &mut egui::Ui, x, kind, tip: &str, selected| {
            style::icon_state(
                ui,
                tip,
                Rect::from_center_size(
                    base + Vec2::new(origin(rect.width()) + x, 22.0),
                    Vec2::splat(32.0),
                ),
                kind,
                Color32::from_gray(210),
                tip,
                selected,
            )
            .clicked()
        };
        if button(ui, 16.0, Icon::Menu, self.tr("ファイル", "File"), false) {
            self.toggle(Popup::File);
        }
        if button(ui, 50.0, Icon::Book, self.tr("辞書", "Dictionary"), false) {
            self.dictionary = true;
            self.popup = None;
        }
        if button(
            ui,
            84.0,
            Icon::Find,
            self.tr("検索と置換", "Find and replace"),
            false,
        ) {
            self.command(Command::FindReplace, ui.ctx());
        }
        for (x, kind, popup, ja, en) in [
            (
                178.0,
                Icon::Speed,
                Popup::Speed,
                "全体の速さ",
                "Global speed",
            ),
            (
                222.0,
                Icon::Pitch,
                Popup::Pitch,
                "全体のピッチ",
                "Global pitch",
            ),
            (
                266.0,
                Icon::Pause,
                Popup::Pause,
                "全体のポーズ長さ",
                "Global pause",
            ),
            (
                310.0,
                Icon::Volume,
                Popup::Volume,
                "全体の音量",
                "Global volume",
            ),
            (
                354.0,
                Icon::Emotion,
                Popup::Emotion,
                "各ボイスの補正",
                "Narrator voice settings",
            ),
        ] {
            if button(ui, x, kind, self.tr(ja, en), false) {
                self.toggle(popup);
            }
        }
        style::label(
            ui,
            base + Vec2::new(origin(rect.width()) + 430.0, 22.0),
            timestamp(self.playhead),
            20.0,
            Color32::from_gray(230),
        );
        for (offset, kind, command) in [
            (548.0, Icon::Previous, Command::SkipStart),
            (580.0, Icon::Play, Command::PlayPause),
            (612.0, Icon::Next, Command::SkipEnd),
            (652.0, Icon::Loop, Command::ToggleLoop),
            (744.0, Icon::Undo, Command::Undo),
            (778.0, Icon::Redo, Command::Redo),
        ] {
            let selected = (command == Command::ToggleLoop && self.loop_playback)
                || (command == Command::PlayPause
                    && self.transport == crate::app::transport::Transport::Playing);
            if button(ui, offset, kind, self.tr("", command.label()), selected) {
                self.command(command, ui.ctx());
            }
        }
        if rect.width() > 1130.0 {
            style::label(
                ui,
                Pos2::new(rect.right() - 196.0, bar.center().y),
                self.tr("音量", "Volume"),
                14.0,
                Color32::WHITE,
            );
            let rail = Rect::from_min_size(
                Pos2::new(rect.right() - 108.0, bar.center().y - 6.0),
                Vec2::new(84.0, 12.0),
            );
            let response = ui.interact(
                rail.expand(6.0),
                ui.id().with("master-volume"),
                Sense::click_and_drag(),
            );
            if response.dragged() || response.clicked() {
                if let Some(p) = response.interact_pointer_pos() {
                    self.global_volume =
                        ((p.x - rail.left()) / rail.width() * 100.0).clamp(0.0, 100.0);
                }
            }
            let x = rail.left() + self.global_volume / 100.0 * rail.width();
            ui.painter().line_segment(
                [rail.left_center(), rail.right_center()],
                Stroke::new(1.5, Color32::WHITE),
            );
            ui.painter().circle_stroke(
                Pos2::new(x, rail.center().y),
                7.0,
                Stroke::new(1.5, Color32::WHITE),
            );
        }
    }
}

/// Toolbar control center.
pub(crate) fn origin(width: f32) -> f32 {
    ((width - 776.0) / 2.0).trunc()
}
