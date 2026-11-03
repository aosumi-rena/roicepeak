use super::{TEXT, font, slider_colored_mixed};
use eframe::egui::{self, Color32, Rect, Response, Sense, Vec2};
use roicepeak::scalar_layers::ValuePolicy;

pub fn set_value_policy(ctx: &egui::Context, value: ValuePolicy) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("control-value-policy"), value));
}

pub(super) fn policy(ui: &egui::Ui) -> ValuePolicy {
    ui.ctx()
        .data(|data| data.get_temp(egui::Id::new("control-value-policy")))
        .unwrap_or_default()
}

pub fn nonnegative<R>(
    ui: &mut egui::Ui,
    restrict: bool,
    show: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    if !restrict {
        return show(ui);
    }
    let key = egui::Id::new("nonnegative-control");
    struct Restore(egui::Context, Option<bool>);
    impl Drop for Restore {
        fn drop(&mut self) {
            self.0.data_mut(|data| {
                let key = egui::Id::new("nonnegative-control");
                if let Some(value) = self.1 {
                    data.insert_temp(key, value);
                } else {
                    data.remove::<bool>(key);
                }
            });
        }
    }
    let original = ui.ctx().data(|data| data.get_temp::<bool>(key));
    let _restore = Restore(ui.ctx().clone(), original);
    ui.ctx().data_mut(|data| data.insert_temp(key, true));
    show(ui)
}

pub(super) fn limits(ui: &egui::Ui, min: f32, max: f32) -> (f32, f32) {
    let (low, high) = policy(ui).limits(min / 100.0, max / 100.0);
    let nonnegative = ui.ctx().data(|data| {
        data.get_temp::<bool>(egui::Id::new("nonnegative-control"))
            .unwrap_or(false)
    });
    (
        if nonnegative { low.max(0.0) } else { low } * 100.0,
        high * 100.0,
    )
}

#[derive(Clone, Default)]
struct Editor {
    open: bool,
    text: String,
    composing: bool,
}

pub fn parameter_row(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    enabled: bool,
    name: &str,
    color: Color32,
) -> Response {
    parameter_row_padded(
        ui, id, rect, value, min, max, default, enabled, name, color, 0.0,
    )
}

pub fn parameter_row_padded(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    enabled: bool,
    name: &str,
    color: Color32,
    padding: f32,
) -> Response {
    parameter_row_impl(
        ui,
        id,
        rect,
        value,
        min,
        max,
        default,
        enabled,
        name,
        color,
        padding,
        false,
        "Multiple Values",
    )
}

pub fn parameter_row_mixed(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    enabled: bool,
    name: &str,
    color: Color32,
    mixed: bool,
    mixed_label: &str,
) -> Response {
    parameter_row_impl(
        ui,
        id,
        rect,
        value,
        min,
        max,
        default,
        enabled,
        name,
        color,
        0.0,
        mixed,
        mixed_label,
    )
}

fn parameter_row_impl(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    enabled: bool,
    name: &str,
    color: Color32,
    padding: f32,
    mixed: bool,
    mixed_label: &str,
) -> Response {
    let identity = ui.id().with(id);
    let text_id = identity.with("value-editor");
    let state_id = identity.with("value-editor-state");
    let mut editor: Editor = ui
        .ctx()
        .data_mut(|data| data.get_temp(state_id))
        .unwrap_or_default();
    let label_rect = Rect::from_min_size(rect.min, Vec2::new(72.0 + padding, rect.height()));
    let mut response = ui.interact(
        label_rect,
        identity.with("label"),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, enabled, name));
    let mut initial = false;
    let mut committed = false;
    if enabled && response.clicked() && !editor.open {
        editor.open = true;
        editor.text = format!("{:.0}", value.trunc());
        initial = true;
        ui.memory_mut(|memory| memory.request_focus(text_id));
    }
    if editor.open {
        let focused = initial || ui.memory(|memory| memory.has_focus(text_id));
        let (enter, escape) = ui.input_mut(|input| {
            if !focused {
                return (false, false);
            }
            let was_composing = editor.composing;
            let mut ime_event = false;
            for event in &input.events {
                if let egui::Event::Ime(event) = event {
                    ime_event = true;
                    match event {
                        egui::ImeEvent::Preedit { text, .. } => editor.composing = !text.is_empty(),
                        egui::ImeEvent::Commit(_) => editor.composing = false,
                        _ => {}
                    }
                }
            }
            if was_composing || editor.composing || ime_event {
                return (false, false);
            }
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        ui.painter().rect_filled(label_rect, 4, Color32::WHITE);
        ui.painter().rect_stroke(
            label_rect.shrink(1.0),
            4,
            egui::Stroke::new(1.0, Color32::from_gray(192)),
            egui::StrokeKind::Middle,
        );
        let field = ui.put(
            label_rect,
            egui::TextEdit::singleline(&mut editor.text)
                .id(text_id)
                .char_limit(12)
                .font(font(14.0))
                .text_color(TEXT)
                .margin(egui::Margin::symmetric(4, 0))
                .vertical_align(egui::Align::Center)
                .frame(egui::Frame::NONE),
        );
        editor.text.retain(|ch| "+-0123456789.".contains(ch));
        if initial {
            field.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), text_id) {
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(editor.text.chars().count()),
                    )));
                state.store(ui.ctx(), text_id);
            }
        }
        if escape || enter || (!initial && field.lost_focus() && !editor.composing) {
            if !escape {
                if let Ok(parsed) = editor
                    .text
                    .trim()
                    .trim_end_matches('%')
                    .trim()
                    .parse::<f32>()
                {
                    if parsed.is_finite() {
                        committed = true;
                        let (low, high) = limits(ui, min, max);
                        let parsed = ((parsed / 5.0).round() * 5.0).clamp(low, high);
                        if *value != parsed || mixed {
                            *value = parsed;
                            response.mark_changed();
                        }
                    }
                }
            }
            editor.open = false;
            if field.has_focus() {
                ui.memory_mut(|memory| memory.surrender_focus(text_id));
            }
        }
        // Editing the text draft is not an authored parameter change.
        // Only a successful commit above reports the edit to bulk consumers.
    } else {
        let hovered = enabled && ui.rect_contains_pointer(rect);
        let text = if hovered {
            format!("{:.0} %", value.trunc())
        } else {
            name.to_owned()
        };
        ui.painter().with_clip_rect(label_rect).text(
            label_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            text,
            font(14.0),
            if enabled {
                TEXT
            } else {
                Color32::from_gray(180)
            },
        );
    }
    ui.ctx().data_mut(|data| data.insert_temp(state_id, editor));
    let track = Rect::from_min_size(
        egui::pos2(rect.left() + 76.0 + padding, rect.center().y - 11.0),
        Vec2::new((rect.width() - 76.0 - padding).max(20.0), 22.0),
    );
    response.union(slider_colored_mixed(
        ui,
        identity.with("track"),
        track,
        value,
        min,
        max,
        default,
        enabled,
        name,
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 76),
        mixed,
        mixed && !ui.rect_contains_pointer(rect),
        mixed_label,
        committed,
    ))
}
