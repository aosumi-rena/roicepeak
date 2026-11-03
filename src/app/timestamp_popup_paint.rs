use crate::app::style;
use eframe::egui::{self, Color32, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

pub(super) fn contrast(color: Color32, amount: f32) -> Color32 {
    let [red, green, blue, alpha] = color.to_srgba_unmultiplied();
    let (r, g, b) = (
        red as f32 / 255.0,
        green as f32 / 255.0,
        blue as f32 / 255.0,
    );
    let brightness = ((g * g * 0.691 + r * r * 0.241) + b * b * 0.068).sqrt();
    let target = if brightness >= 0.5 { 0 } else { 255 };
    let overlay_alpha = (amount * 255.0).round_ties_even() as i32;
    if alpha == 0 {
        return Color32::from_rgba_unmultiplied(target, target, target, overlay_alpha as u8);
    }
    let inverse = 255 - overlay_alpha;
    let out_alpha = 255 - (((255 - alpha as i32) * inverse) >> 8);
    let weight = alpha as i32 * inverse / out_alpha;
    let blend =
        |channel: u8| (target as i32 + (((channel as i32 - target as i32) * weight) >> 8)) as u8;
    Color32::from_rgba_unmultiplied(blend(red), blend(green), blend(blue), out_alpha as u8)
}

pub(super) fn interactive_color(color: Color32, response: &Response) -> Color32 {
    if response.is_pointer_button_down_on() {
        contrast(color, 0.2)
    } else if response.hovered() {
        contrast(color, 0.05)
    } else {
        color
    }
}

pub(super) fn field_background(ui: &egui::Ui, rect: Rect, focused: bool, hovered: bool) {
    let color = if hovered && !focused {
        Color32::from_black_alpha(5)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 4, color);
}

pub(super) fn field_outline(ui: &egui::Ui, rect: Rect, focused: bool, hovered: bool) {
    let (rect, width, color) = if focused {
        (rect.expand(1.0), 2.0, Color32::from_gray(125))
    } else {
        (
            rect.shrink(0.5),
            1.0,
            Color32::from_gray(if hovered { 175 } else { 221 }),
        )
    };
    ui.painter()
        .rect_stroke(rect, 4, Stroke::new(width, color), StrokeKind::Middle);
}

pub(super) fn button(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    text: &str,
    color: Color32,
    text_color: Color32,
) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
    let fill = interactive_color(color, &response);
    if color.to_srgba_unmultiplied()[..3] == [255, 255, 255] {
        let inner = rect.shrink(1.5);
        ui.painter().rect_filled(inner, 3, fill);
        ui.painter()
            .rect_stroke(inner, 3, Stroke::new(1.0, text_color), StrokeKind::Middle);
    } else {
        ui.painter().rect_filled(rect.shrink(0.5), 4, fill);
    }
    ui.painter()
        .with_clip_rect(rect.shrink2(Vec2::new(4.0, 4.0)))
        .text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            style::font(23.04 / 1.48),
            text_color,
        );
    response
}
