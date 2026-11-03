use crate::app::{i18n, native_icons, style};
use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke, Vec2};

pub(super) fn search(
    ui: &mut egui::Ui,
    origin: Pos2,
    value: &mut String,
    locale: crate::app::i18n::Locale,
) {
    let rect = Rect::from_min_size(origin + Vec2::new(16.0, 16.0), Vec2::new(176.0, 32.0));
    ui.painter().rect_filled(rect, 3, Color32::WHITE);
    ui.painter().rect_stroke(
        rect,
        3,
        Stroke::new(1.0, Color32::from_gray(221)),
        egui::StrokeKind::Inside,
    );
    let edit_rect = Rect::from_min_max(rect.min, rect.max - Vec2::new(30.0, 0.0));
    ui.put(
        edit_rect,
        egui::TextEdit::singleline(value)
            .id(ui.id().with("dictionary-search"))
            .hint_text(i18n::tr(locale, "検索", "Search"))
            .font(style::font(14.0))
            .text_color(style::TEXT)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::symmetric(6, 0))
            .vertical_align(egui::Align::Center),
    );
    let icon = Rect::from_center_size(
        rect.right_center() - Vec2::new(16.0, 0.0),
        Vec2::splat(24.0),
    );
    if value.is_empty() {
        native_icons::paint_dialog(ui, icon, 48);
    } else {
        let clear = ui.interact(
            icon,
            ui.id().with("dictionary-search-clear"),
            Sense::click(),
        );
        clear.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                i18n::tr(locale, "クリア", "Clear"),
            )
        });
        native_icons::paint_dialog(ui, icon, 49);
        if clear.clicked() {
            value.clear();
        }
    }
}

pub(super) fn entry_row(ui: &mut egui::Ui, index: usize, text: &str, selected: bool) -> Response {
    let (_, rect) = ui.allocate_space(Vec2::new(208.0, 34.0));
    let response = ui.interact(
        rect,
        ui.id().with(("dictionary-entry", index)),
        Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, text)
    });
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            3,
            if selected {
                Color32::from_gray(222)
            } else {
                Color32::from_gray(232)
            },
        );
    }
    ui.painter().with_clip_rect(rect).text(
        rect.left_center() + Vec2::new(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        style::font(14.0),
        style::TEXT,
    );
    response
}
