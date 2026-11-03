use crate::app::{native_menu, style};
#[path = "generic_combo.rs"]
mod combo_interaction;
pub(crate) use combo_interaction::{Row as ComboRow, interact as combo_input};
use eframe::egui::{self, Color32, Id, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

pub(crate) const HEADING: Color32 = Color32::from_rgb(71, 78, 132);

pub(crate) fn alert_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(Color32::WHITE)
        .shadow(egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(48),
        })
}

pub(crate) fn floating_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(Color32::WHITE)
        .corner_radius(6)
        .stroke(Stroke::new(1.0, style::BORDER))
        .shadow(egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(32),
        })
}

pub(crate) fn text_field(
    ui: &mut egui::Ui,
    rect: Rect,
    id: Id,
    value: &mut String,
    hint: &str,
    centered: bool,
) -> Response {
    text_field_with_limit(ui, rect, id, value, hint, centered, 4096)
}

pub(crate) fn text_field_with_limit(
    ui: &mut egui::Ui,
    rect: Rect,
    id: Id,
    value: &mut String,
    hint: &str,
    centered: bool,
    limit: usize,
) -> Response {
    ui.painter().rect_filled(rect, 3, Color32::WHITE);
    let response = ui.put(
        rect,
        egui::TextEdit::singleline(value)
            .id(id)
            .hint_text(hint)
            .char_limit(limit)
            .font(style::font(14.0))
            .text_color(style::TEXT)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::symmetric(6, 0))
            .horizontal_align(if centered {
                egui::Align::Center
            } else {
                egui::Align::Min
            })
            .vertical_align(egui::Align::Center),
    );
    ui.painter().rect_stroke(
        rect,
        3,
        Stroke::new(
            1.0,
            Color32::from_gray(if response.has_focus() { 153 } else { 221 }),
        ),
        StrokeKind::Inside,
    );
    response
}

pub(crate) fn combo_face(ui: &egui::Ui, rect: Rect, caption: &str) {
    combo_face_colored(ui, rect, caption, style::TEXT);
}

pub(crate) fn combo_face_colored(ui: &egui::Ui, rect: Rect, caption: &str, color: Color32) {
    ui.painter().rect_filled(rect, 4, Color32::WHITE);
    ui.painter().rect_stroke(
        rect.shrink(0.5),
        4,
        Stroke::new(1.0, Color32::from_black_alpha(31)),
        StrokeKind::Middle,
    );
    let label = Rect::from_min_max(rect.min + Vec2::splat(1.0), rect.max - Vec2::new(29.0, 1.0));
    let text = Rect::from_min_max(
        label.min + Vec2::new(5.0, 1.0),
        label.max - Vec2::new(5.0, 1.0),
    );
    ui.painter()
        .with_clip_rect(text.intersect(ui.clip_rect()))
        .text(
            text.left_center(),
            egui::Align2::LEFT_CENTER,
            caption,
            style::font(20.880_001 / 1.48),
            color,
        );
    let at = egui::pos2(rect.right() - 20.0, rect.center().y - 3.5);
    let mut mesh = egui::Mesh::default();
    for (x, y) in [
        (1.0, 0.0),
        (6.0, 4.0),
        (11.0, 0.0),
        (12.0, 1.0),
        (6.5, 6.0),
        (5.5, 6.0),
        (0.0, 1.0),
    ] {
        mesh.colored_vertex(at + Vec2::new(x, y), Color32::from_gray(179));
    }
    for [a, b, c] in [[1, 2, 3], [1, 3, 4], [1, 4, 5], [1, 5, 6], [1, 6, 0]] {
        mesh.add_triangle(a, b, c);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

pub(crate) fn combo<T: Clone + PartialEq>(
    ui: &mut egui::Ui,
    rect: Rect,
    id: Id,
    value: &mut T,
    choices: &[(T, &str)],
) -> bool {
    let caption = choices
        .iter()
        .find(|(key, _)| key == value)
        .map_or("", |(_, text)| *text);
    let response = ui.interact(rect, id.with("control"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), caption)
    });
    combo_face(ui, rect, caption);
    let open = egui::Popup::is_id_open(ui.ctx(), id);
    let input = combo_input(ui, &response, open, true, || {
        (
            choices
                .iter()
                .position(|(key, _)| key == value)
                .map(|i| i as u64 + 1),
            (0..choices.len())
                .map(|i| ComboRow {
                    id: Some(i as u64 + 1),
                    enabled: true,
                })
                .collect(),
        )
    });
    let mut changed = false;
    if let Some(index) = input.selected.and_then(|id| usize::try_from(id - 1).ok()) {
        if let Some((key, _)) = choices.get(index) {
            changed = *value != *key;
            *value = key.clone();
        }
    }
    if input.toggle && open {
        native_menu::close(ui.ctx(), id);
        return changed;
    }
    if !ui.is_enabled() || (!input.toggle && !open) {
        return changed;
    }
    let current = choices.iter().position(|(key, _)| key == value);
    let items = choices
        .iter()
        .enumerate()
        .map(|(index, (_, text))| native_menu::Item::CheckedAction {
            text,
            shortcut: String::new(),
            enabled: true,
            checked: Some(index) == current,
            action: index,
        })
        .collect::<Vec<_>>();
    let outcome = native_menu::show_selector_at_width(ui, id, rect, &items, rect.width(), current);
    if let Some(index) = outcome.action {
        changed |= *value != choices[index].0;
        *value = choices[index].0.clone();
    }
    changed
}

pub(crate) fn toggle(
    ui: &mut egui::Ui,
    rect: Rect,
    id: Id,
    value: &mut bool,
    label: &str,
) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    if response.clicked() && ui.is_enabled() {
        *value = !*value;
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label)
    });
    let track = Rect::from_center_size(rect.center(), Vec2::new(32.0, 16.0));
    ui.painter().rect_filled(
        track,
        8,
        if *value {
            Color32::from_rgb(79, 84, 143)
        } else {
            Color32::from_gray(170)
        },
    );
    ui.painter().circle_filled(
        egui::pos2(
            if *value {
                track.right() - 8.0
            } else {
                track.left() + 8.0
            },
            track.center().y,
        ),
        6.0,
        Color32::WHITE,
    );
    response
}

pub(crate) fn section(ui: &egui::Ui, rect: Rect, text: &str) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), style::font(14.0), style::TEXT);
    let end = rect.left() + galley.size().x + 16.0;
    ui.painter().galley(
        rect.left_center() - Vec2::new(0.0, galley.size().y * 0.5),
        galley,
        style::TEXT,
    );
    if end < rect.right() {
        ui.painter().line_segment(
            [egui::pos2(end, rect.center().y), rect.right_center()],
            Stroke::new(1.0, Color32::from_gray(225)),
        );
    }
}
