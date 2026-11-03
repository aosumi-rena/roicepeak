use crate::app::{
    document::state::{DurationMode, DurationState},
    style,
};
use eframe::egui::{self, Color32, Rect, Stroke, Vec2};

pub(super) fn bar(
    ui: &egui::Ui,
    card: Rect,
    cell: Rect,
    duration: DurationState,
    color: Color32,
    word_hovered: bool,
    selected: bool,
) {
    if duration.mode != DurationMode::Relative {
        return;
    }
    let bar = Rect::from_center_size(
        egui::pos2(cell.center().x, card.top() + 87.0),
        Vec2::new((duration.value * 28.0) as f32, 47.0),
    );
    if selected {
        ui.painter().rect_filled(
            bar,
            2,
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 51),
        );
    } else {
        let color = if word_hovered {
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 153)
        } else {
            style::BORDER
        };
        ui.painter()
            .rect_stroke(bar, 0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
    }
    let hovered = word_hovered
        && ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| p.x >= cell.left() && p.x < cell.right());
    if !selected && !hovered {
        return;
    }
    let cy = bar.center().y;
    let ink = if selected {
        color
    } else {
        Color32::from_rgba_unmultiplied(0, 0, 0, 89)
    };
    let stroke = Stroke::new(2.0, ink);
    ui.painter().add(egui::Shape::line(
        vec![
            egui::pos2(bar.left() + 8.0, cy - 4.0),
            egui::pos2(bar.left() + 4.0, cy),
            egui::pos2(bar.left() + 8.0, cy + 4.0),
        ],
        stroke,
    ));
    ui.painter().line_segment(
        [
            egui::pos2(bar.right() - 4.0, cy),
            egui::pos2(bar.left() + 4.0, cy),
        ],
        stroke,
    );
    ui.painter().add(egui::Shape::line(
        vec![
            egui::pos2(bar.right() - 8.0, cy - 4.0),
            egui::pos2(bar.right() - 4.0, cy),
            egui::pos2(bar.right() - 8.0, cy + 4.0),
        ],
        stroke,
    ));
}
