use crate::app::{i18n, style};
use eframe::egui::{self, Color32, Rect, Sense, Stroke, Vec2};

#[derive(Default)]
pub(super) struct State {
    held: bool,
}

impl State {
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        value: &mut u8,
        locale: i18n::Locale,
    ) {
        let id = ui.id().with("dictionary-priority");
        let response = ui.interact(rect, id, Sense::click_and_drag());
        let (position, pressed, down) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
            )
        });
        if !ui.is_enabled() {
            self.held = false;
        }
        if ui.is_enabled() && pressed && response.is_pointer_button_down_on() {
            self.held = true;
        }
        if ui.is_enabled() && self.held && down {
            if let Some(p) = position.filter(|p| p.x.is_finite()) {
                *value = at(rect, p.x);
            }
        }
        if !down {
            self.held = false;
        }
        let hover = if response.hovered() {
            position.map(|p| at(rect, p.x))
        } else {
            None
        };
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Slider,
                ui.is_enabled(),
                i18n::format(locale, "Priority {}", &[&value.to_string()]),
            )
        });
        let y = rect.center().y;
        let painter = ui.painter();
        painter.line_segment(
            [
                egui::pos2(rect.left() + 14.0, y),
                egui::pos2(rect.right() - 14.0, y),
            ],
            Stroke::new(1.0, Color32::from_gray(224)),
        );
        for v in 1..=10u8 {
            let center = egui::pos2(
                rect.left() + 14.0 + f32::from(v - 1) * (rect.width() - 28.0) / 9.0,
                y,
            );
            painter.circle_filled(center, 4.0, Color32::WHITE);
            painter.circle_filled(
                center,
                3.0,
                Color32::from_gray(if hover == Some(v) { 169 } else { 196 }),
            );
            if *value == v {
                let body = Rect::from_center_size(center, Vec2::splat(28.0));
                painter.add(
                    egui::epaint::Shadow {
                        offset: [0, 2],
                        blur: 6,
                        spread: 0,
                        color: Color32::from_rgba_unmultiplied(6, 14, 26, 38),
                    }
                    .as_shape(body, 14),
                );
                painter.circle_filled(center, 14.0, Color32::from_gray(102));
                painter.circle_filled(
                    center,
                    13.0,
                    Color32::from_gray(if self.held {
                        237
                    } else if hover == Some(v) {
                        247
                    } else {
                        255
                    }),
                );
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    v.to_string(),
                    style::font(17.28 / 1.48),
                    Color32::from_gray(102),
                );
            }
        }
    }
}

fn at(rect: Rect, x: f32) -> u8 {
    let local = x - rect.left();
    let fraction = f64::from(local - 14.0) / f64::from(rect.width() - 28.0);
    (1.0 + (fraction.clamp(0.0, 1.0) * 9.0).round()).clamp(1.0, 10.0) as u8
}
