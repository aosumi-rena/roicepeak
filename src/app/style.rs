use eframe::egui::{
    self, Color32, FontFamily, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};
#[path = "style_control.rs"]
mod control;
#[path = "style_range.rs"]
mod range;
pub use control::{
    nonnegative, parameter_row, parameter_row_mixed, parameter_row_padded, set_value_policy,
};
#[path = "style_button.rs"]
mod button;
pub use button::{button_paint, text_button};

pub const BACKGROUND: Color32 = Color32::from_rgb(245, 245, 245);
pub const BAR: Color32 = Color32::from_rgb(93, 94, 98);
pub const TEXT: Color32 = Color32::from_rgb(99, 99, 99);
pub const BORDER: Color32 = Color32::from_rgb(210, 210, 210);

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn label(ui: &Ui, position: Pos2, text: impl Into<String>, size: f32, color: Color32) {
    let text = text.into();
    let region = Rect::from_min_size(
        position - Vec2::new(0.0, size * 0.5),
        Vec2::new((text.chars().count() as f32 * size).max(size), size),
    );
    ui.interact(
        region,
        ui.id()
            .with(("label", position.x.to_bits(), position.y.to_bits())),
        Sense::hover(),
    )
    .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &text));
    ui.painter()
        .text(position, egui::Align2::LEFT_CENTER, text, font(size), color);
}
pub fn card(ui: &Ui, rect: Rect) {
    card_radius(ui, rect, 6);
}
pub fn card_radius(ui: &Ui, rect: Rect, radius: u8) {
    ui.painter().rect_filled(
        rect.translate(Vec2::new(0.0, 1.0)),
        radius,
        Color32::from_gray(226),
    );
    ui.painter().rect_filled(rect, radius, Color32::WHITE);
    ui.painter()
        .rect_stroke(rect, radius, Stroke::new(1.0, BORDER), StrokeKind::Inside);
}

#[derive(Clone, Copy)]
pub enum Icon {
    Menu,
    Book,
    Find,
    Volume,
    Loop,
    Speed,
    Pitch,
    Pause,
    Emotion,
    Previous,
    Play,
    Next,
    Undo,
    Redo,
    Accent,
    Intonation,
    Duration,
    Delete,
    BlockLoop,
    PresetMore,
    Chevron,
}

pub fn icon(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    kind: Icon,
    color: Color32,
    tooltip: &str,
) -> Response {
    icon_state(ui, id, rect, kind, color, tooltip, false)
}

pub fn icon_state(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    kind: Icon,
    _color: Color32,
    tooltip: &str,
    selected: bool,
) -> Response {
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    let response = crate::app::root_tooltip::attach(response, tooltip);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    let is_mode = matches!(kind, Icon::Accent | Icon::Intonation | Icon::Duration);
    let over = response.hovered() || response.is_pointer_button_down_on();
    if !is_mode {
        let background = if matches!(kind, Icon::Delete | Icon::BlockLoop | Icon::PresetMore) {
            let alpha = if selected { 51 } else { 0 } + if over { 20 } else { 0 };
            Color32::from_black_alpha(alpha)
        } else {
            let alpha = match (selected, over) {
                (false, false) => 0,
                (false, true) => 13,
                (true, false) => 51,
                (true, true) => 76,
            };
            Color32::from_white_alpha(alpha)
        };
        ui.painter().rect_filled(rect, 4, background);
    }
    crate::app::native_icons::paint(ui, rect, kind, selected, over);
    response
}

pub fn draw_icon(ui: &Ui, c: Pos2, kind: Icon, _color: Color32) {
    if matches!(kind, Icon::Chevron) {
        // Triangulate the concave chevron from its inward corner.
        let points = [
            (1.0, 0.0),
            (4.0, 2.0),
            (7.0, 0.0),
            (8.0, 1.0),
            (4.5, 4.0),
            (3.5, 4.0),
            (0.0, 1.0),
        ];
        let mut mesh = egui::Mesh::default();
        for (x, y) in points {
            mesh.colored_vertex(c + Vec2::new(x - 4.0, y - 2.0), Color32::from_gray(179));
        }
        for [a, b, d] in [[1, 2, 3], [1, 3, 4], [1, 4, 5], [1, 5, 6], [1, 6, 0]] {
            mesh.add_triangle(a, b, d);
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        return;
    }
    crate::app::native_icons::paint(
        ui,
        Rect::from_center_size(c, Vec2::splat(32.0)),
        kind,
        false,
        false,
    );
}

pub fn slider_colored(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    enabled: bool,
    accessibility_label: &str,
    fill: Color32,
) -> Response {
    slider_colored_mixed(
        ui,
        id,
        rect,
        value,
        min,
        max,
        default,
        enabled,
        accessibility_label,
        fill,
        false,
        false,
        "",
        false,
    )
}

fn slider_colored_mixed(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    enabled: bool,
    accessibility_label: &str,
    fill: Color32,
    mixed: bool,
    mixed_caption: bool,
    mixed_label: &str,
    reframe_range: bool,
) -> Response {
    #[derive(Clone, Copy)]
    struct Grab {
        value: f64,
        offset: f64,
        range: range::Range,
    }
    let (allowed_min, allowed_max) = control::limits(ui, min, max);
    let display = range::Range {
        min: f64::from(allowed_min),
        max: f64::from(allowed_max),
    };
    let before = *value;
    let mut edit_intent = false;
    let identity = ui.id().with(id);
    let grab_id = identity.with("grab");
    if reframe_range
        || !enabled
        || ui
            .data(|data| data.get_temp::<Grab>(grab_id))
            .is_some_and(|grab| grab.range != display)
    {
        ui.data_mut(|data| data.remove::<Grab>(grab_id));
    }
    let mut response = ui.interact(
        rect,
        identity,
        if enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::slider(enabled, *value as f64, accessibility_label));
    if enabled && response.clicked() {
        response.request_focus();
    }
    if enabled && response.has_focus() {
        ui.input(|i| {
            edit_intent = [
                egui::Key::ArrowLeft,
                egui::Key::ArrowDown,
                egui::Key::ArrowRight,
                egui::Key::ArrowUp,
                egui::Key::Home,
                egui::Key::End,
            ]
            .iter()
            .any(|&key| i.key_pressed(key));
            if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowDown) {
                *value -= 5.0;
            }
            if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowUp) {
                *value += 5.0;
            }
            if i.key_pressed(egui::Key::Home) {
                *value = display.min as f32;
            }
            if i.key_pressed(egui::Key::End) {
                *value = display.max as f32;
            }
        });
        *value = value.clamp(allowed_min, allowed_max);
    }
    if enabled
        && (response.is_pointer_button_down_on()
            || response.clicked()
            || response.dragged()
            || response.drag_stopped())
    {
        if let Some(position) = response.interact_pointer_pos() {
            edit_intent = true;
            let span = f64::from((rect.width() - 21.0).max(1.0));
            let start = f64::from(rect.left() + 10.0);
            let origin = ui
                .input(|input| input.pointer.press_origin())
                .unwrap_or(position);
            let grab = ui.data_mut(|data| {
                *data.get_temp_mut_or_insert_with(grab_id, || {
                    let center = (start + span * f64::from(display.ratio(*value))).round();
                    let on_thumb = (f64::from(origin.x) - center).abs() < 10.0
                        && (origin.y - rect.center().y).abs() < 10.0;
                    Grab {
                        value: f64::from(*value),
                        range: display,
                        offset: if on_thumb {
                            center - f64::from(origin.x)
                        } else {
                            0.0
                        },
                    }
                })
            });
            let mut next = grab
                .range
                .at(((f64::from(position.x) + grab.offset - start) / span).clamp(0.0, 1.0));
            if ui.input(|input| input.modifiers.ctrl) {
                next += (grab.value - next) * 0.8;
            }
            *value = ((next / 5.0).round() * 5.0)
                .clamp(grab.range.min, grab.range.max)
                .clamp(f64::from(allowed_min), f64::from(allowed_max)) as f32;
        }
    }
    if !response.is_pointer_button_down_on() {
        ui.data_mut(|data| data.remove::<Grab>(grab_id));
    }
    if enabled && response.double_clicked() {
        edit_intent = true;
        *value = default.clamp(allowed_min, allowed_max);
        ui.data_mut(|data| data.remove::<Grab>(grab_id));
    }
    if *value != before || (mixed && edit_intent) {
        response.mark_changed();
    }
    let rail = rect.shrink2(Vec2::new(0.0, 1.0));
    ui.painter()
        .rect_filled(rail, 2, Color32::from_rgb(246, 246, 246));
    let ratio = display.ratio(*value);
    let x = rail.left() + 10.0 + ratio * (rail.width() - 21.0).max(1.0);
    let origin = if display.min < 0.0 {
        rail.left() + 10.0 + display.ratio(0.0) * (rail.width() - 21.0)
    } else {
        rail.left()
    };
    if !mixed {
        ui.painter().rect_filled(
            Rect::from_min_max(
                Pos2::new(origin.min(x), rail.top()),
                Pos2::new(origin.max(x), rail.bottom()),
            ),
            2,
            if enabled {
                fill
            } else {
                Color32::from_gray(213)
            },
        );
    }
    ui.painter().rect_stroke(
        rail,
        2,
        Stroke::new(1.0, Color32::from_gray(228)),
        StrokeKind::Inside,
    );
    if mixed_caption {
        ui.painter().with_clip_rect(rail).text(
            rail.center(),
            egui::Align2::CENTER_CENTER,
            mixed_label,
            font(17.0 / 1.48),
            Color32::from_gray(179),
        );
        return response;
    }
    let thumb = Rect::from_center_size(
        Pos2::new(
            x.clamp(rail.left() + 10.0, rail.right() - 11.0),
            rail.center().y,
        ),
        Vec2::new(20.0, 21.0),
    );
    ui.painter().rect_filled(
        thumb.translate(Vec2::new(0.0, 1.0)),
        2,
        Color32::from_gray(220),
    );
    ui.painter().rect_filled(thumb, 2, Color32::WHITE);
    ui.painter().rect_stroke(
        thumb,
        2,
        Stroke::new(1.0, Color32::from_gray(208)),
        StrokeKind::Inside,
    );
    response
}
