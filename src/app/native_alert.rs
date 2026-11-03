//! Shared editor confirmation and message dialogs.
use crate::app::style;
use eframe::egui::{self, Color32, Id, Rect, Sense, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Information,
    Error,
}

#[derive(Clone)]
struct Focus {
    previous: Option<Id>,
}

pub(crate) fn begin(ctx: &egui::Context, id: Id) {
    let state_id = id.with("focus");
    if ctx.data(|data| data.get_temp::<Focus>(state_id)).is_none() {
        let previous = ctx.memory(|memory| memory.focused());
        ctx.data_mut(|data| data.insert_temp(state_id, Focus { previous }));
        ctx.memory_mut(|memory| memory.request_focus(id.with("keyboard")));
    }
}

fn color(argb: u32) -> Color32 {
    let [a, r, g, b] = argb.to_be_bytes();
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

fn contrast(base: u32, amount: u8) -> u32 {
    let [alpha, red, green, blue] = base.to_be_bytes();
    let brightness = ((red as f32 / 255.0).powi(2) * 0.241
        + (green as f32 / 255.0).powi(2) * 0.691
        + (blue as f32 / 255.0).powi(2) * 0.068)
        .sqrt();
    let other = if brightness >= 0.5 { 0_i32 } else { 255_i32 };
    let out_alpha = 255 - ((255 - alpha as i32) * (255 - amount as i32) >> 8);
    let weight = alpha as i32 * (255 - amount as i32) / out_alpha;
    let channel = |value: u8| (other + (((value as i32 - other) * weight) >> 8)) as u8;
    u32::from_be_bytes([out_alpha as u8, channel(red), channel(green), channel(blue)])
}

pub(crate) fn button(
    ui: &mut egui::Ui,
    id: Id,
    rect: Rect,
    label: &str,
    first: bool,
    tint: u32,
) -> bool {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let mut fill = if first { tint } else { 0xffffffff };
    if response.is_pointer_button_down_on() {
        fill = contrast(fill, 51);
    } else if response.hovered() {
        fill = contrast(fill, 13);
    }
    if first {
        ui.painter().rect_filled(rect.shrink(0.5), 6, color(fill));
    } else {
        ui.painter().rect_filled(rect.shrink(1.5), 6, color(fill));
        ui.painter().rect_stroke(
            rect.shrink(1.5),
            6,
            Stroke::new(1.0, color(0xff1b2b79)),
            StrokeKind::Middle,
        );
    }
    ui.painter().with_clip_rect(rect).text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        style::font(20.88 / 1.48),
        if first {
            Color32::WHITE
        } else {
            color(0xff1b2b79)
        },
    );
    response.clicked()
}

pub(crate) fn show(
    ctx: &egui::Context,
    id: Id,
    title: &str,
    message: &str,
    kind: Kind,
    labels: &[&str],
) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }
    let focus_id = id.with("keyboard");
    let state_id = id.with("focus");
    let initial = ctx.data(|data| data.get_temp::<Focus>(state_id)).is_none();
    begin(ctx, id);
    let available = ctx.viewport_rect().size().max(Vec2::splat(1.0));
    let measured = ctx.fonts_mut(|fonts| {
        [title, message]
            .into_iter()
            .map(|text| {
                fonts
                    .layout_no_wrap(text.to_owned(), style::font(20.88 / 1.48), style::TEXT)
                    .size()
                    .x
            })
            .fold(0.0_f32, f32::max)
    });
    let wrap = ((measured * 20.88).sqrt().trunc() * 2.0 + 300.0)
        .min((available.x * 0.7).trunc())
        .max(1.0);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap;
    job.append(
        title,
        0.0,
        egui::TextFormat {
            font_id: style::font(23.04 / 1.48),
            ..Default::default()
        },
    );
    job.append(
        "\n\n",
        0.0,
        egui::TextFormat {
            font_id: style::font(20.88 / 1.48),
            ..Default::default()
        },
    );
    job.append(
        message,
        0.0,
        egui::TextFormat {
            font_id: style::font(20.88 / 1.48),
            ..Default::default()
        },
    );
    let measured = ctx.fonts_mut(|fonts| fonts.layout_job(job)).size();
    let width = 350.0_f32
        .max(measured.x.trunc() + 120.0)
        .max(40.0 + labels.len() as f32 * 116.0)
        .min((available.x * 0.7).trunc())
        .max(1.0);
    let height = (measured.y.trunc() + 100.0).min((available.y - 50.0).max(1.0));
    let tint = match kind {
        Kind::Information => 0xd1374589,
        Kind::Error => 0xfff26e6e,
    };
    let mut result = None;
    egui::Modal::new(id)
        .backdrop_color(Color32::TRANSPARENT)
        .frame(crate::app::dialog_controls::alert_frame())
        .show(ctx, |ui| {
            let (_, rect) = ui.allocate_space(Vec2::new(width, height));
            let focus = ui.interact(rect, focus_id, Sense::focusable_noninteractive());
            if initial {
                focus.request_focus();
            }
            let at = |x, y, w, h| Rect::from_min_size(rect.min + Vec2::new(x, y), Vec2::new(w, h));
            ui.painter()
                .with_clip_rect(at(16.0, 12.0, (width - 32.0).max(0.0), 29.0))
                .text(
                    rect.min + Vec2::new(16.0, 26.5),
                    egui::Align2::LEFT_CENTER,
                    title,
                    style::font(23.04 / 1.48),
                    color(tint),
                );
            let body = at(
                16.0,
                50.0,
                (width - 32.0).max(0.0),
                (height - 114.0).max(0.0),
            );
            let galley = ui.painter().layout(
                message.to_owned(),
                style::font(20.88 / 1.48),
                Color32::from_gray(102),
                body.width(),
            );
            ui.painter()
                .with_clip_rect(body)
                .galley(body.min, galley, Color32::from_gray(102));
            let start = width - labels.len() as f32 * 104.0 - 12.0;
            for (index, label) in labels.iter().enumerate() {
                if button(
                    ui,
                    id.with(("button", index)),
                    at(start + index as f32 * 104.0, height - 48.0, 100.0, 32.0),
                    label,
                    index == 0,
                    tint,
                ) {
                    result = Some(index);
                }
            }
            let mut mnemonic = [None, None];
            if labels.len() >= 2 {
                for (slot, label) in mnemonic.iter_mut().zip(labels) {
                    *slot = label
                        .chars()
                        .next()
                        .map(|c| c.to_lowercase().collect::<String>());
                }
                if mnemonic[0] == mnemonic[1] {
                    mnemonic[1] = None;
                }
            }
            ui.input_mut(|input| {
                input.events.retain(|event| match event {
                    egui::Event::Key {
                        key: egui::Key::Escape,
                        pressed: true,
                        ..
                    } => {
                        result = Some(labels.len() - 1);
                        false
                    }
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        ..
                    } if labels.len() <= 2 => {
                        result.get_or_insert(0);
                        false
                    }
                    egui::Event::Text(text) if labels.len() >= 2 => {
                        if let Some(index) = mnemonic.iter().position(|key| {
                            key.as_ref().is_some_and(|key| key == &text.to_lowercase())
                        }) {
                            result = Some(index);
                            false
                        } else {
                            true
                        }
                    }
                    _ => true,
                })
            });
        });
    if result.is_some() {
        end(ctx, id);
    }
    result
}

pub(crate) fn end(ctx: &egui::Context, id: Id) {
    let state_id = id.with("focus");
    if let Some(state) = ctx.data(|data| data.get_temp::<Focus>(state_id)) {
        ctx.data_mut(|data| data.remove::<Focus>(state_id));
        ctx.memory_mut(|memory| {
            if let Some(previous) = state.previous {
                memory.request_focus(previous);
            } else {
                memory.surrender_focus(id.with("keyboard"));
            }
        });
    }
}
