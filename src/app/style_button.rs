use eframe::egui::{self, Color32, Rect, Response, Sense, Stroke, StrokeKind};

pub fn text_button(
    ui: &mut egui::Ui,
    rect: Rect,
    text: &str,
    filled: bool,
    enabled: bool,
) -> Response {
    let response = ui.interact(
        rect,
        ui.id().with(("dialog-action", text, filled)),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
    button_paint(ui, rect, &response, filled, enabled);
    let text_color = palette(&response, filled, enabled)[1];
    ui.painter().with_clip_rect(rect).text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        super::font(23.04 / 1.48),
        text_color,
    );
    response
}

pub fn button_paint(ui: &egui::Ui, rect: Rect, response: &Response, filled: bool, enabled: bool) {
    let [fill, _, border] = palette(response, filled, enabled);
    ui.painter().rect_filled(rect, 4, fill);
    if border.a() != 0 {
        ui.painter()
            .rect_stroke(rect, 4, Stroke::new(1.0, border), StrokeKind::Inside);
    }
}

fn palette(response: &Response, filled: bool, enabled: bool) -> [Color32; 3] {
    let state = if !enabled {
        3
    } else if response.is_pointer_button_down_on() {
        2
    } else if response.hovered() {
        1
    } else {
        0
    };
    let colors = if filled {
        [
            [0xcc1b2b79, 0xffffffff, 0x00ffffff],
            [0xd1374589, 0xffffffff, 0x00ffffff],
            [0xd91b2b79, 0xd9ffffff, 0x00ffffff],
            [0xcc1b2b79, 0xffffffff, 0x00ffffff],
        ][state]
    } else {
        [
            [0x00ffffff, 0xff1b2b79, 0xff1b2b79],
            [0x121b2b79, 0xb21b2b79, 0xb21b2b79],
            [0x1f1b2b79, 0xe61b2b79, 0xe61b2b79],
            [0x00ffffff, 0xff1b2b79, 0xff1b2b79],
        ][state]
    };
    colors.map(|argb: u32| {
        let [a, r, g, b] = argb.to_be_bytes();
        let color = Color32::from_rgba_unmultiplied(r, g, b, a);
        if enabled {
            color
        } else {
            color.gamma_multiply(77.0 / 255.0)
        }
    })
}
