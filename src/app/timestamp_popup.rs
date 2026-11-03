use crate::app::{
    document::{Block, OffsetMode},
    editor::EditorApp,
    i18n, style,
};
use eframe::egui::{self, Color32, Id, Pos2, Rect, Sense, Vec2};
#[path = "timestamp_popup_controls.rs"]
mod controls;
#[path = "timestamp_number.rs"]
mod number;
#[path = "timestamp_popup_paint.rs"]
mod paint;

#[derive(Clone)]
struct ValueEdit {
    text: String,
    stable: String,
    composing: bool,
}

#[derive(Clone)]
struct Draft {
    block: u64,
    anchor: Rect,
    color: Color32,
    original_mode: OffsetMode,
    original_value: f64,
    original_start: f64,
    relative: bool,
    end: bool,
    value: f64,
    edit: Option<ValueEdit>,
    opened: u64,
}
fn key() -> Id {
    Id::new("timestamp-popup-state")
}
pub(crate) fn active(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<Draft>(key()).is_some())
}
pub(crate) fn button(ui: &mut egui::Ui, block: &Block, at: Pos2, color: Color32) {
    let rect = Rect::from_min_size(at - Vec2::new(0.0, 13.0), Vec2::new(70.0, 26.0));
    let response = ui.interact(rect, Id::new(("block-timestamp", block.id)), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 4, Color32::from_rgba_unmultiplied(153, 153, 153, 51));
        crate::app::native_icons::paint_dialog(ui, rect, 35);
    } else {
        ui.painter().with_clip_rect(rect).text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            number::absolute(block.timestamp),
            style::font(20.880_001 / 1.48),
            style::TEXT,
        );
    }
    if response.clicked() {
        let existing = ui.ctx().data(|d| d.get_temp::<Draft>(key()));
        if existing.is_some_and(|d| d.block == block.id) {
            close(ui.ctx());
            return;
        }
        let opened = ui.ctx().cumulative_frame_nr();
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                key(),
                Draft {
                    block: block.id,
                    anchor: rect,
                    color,
                    original_mode: block.offset_mode,
                    original_value: block.offset_seconds,
                    original_start: block.timestamp,
                    relative: block.offset_mode != OffsetMode::Absolute,
                    end: block.offset_mode != OffsetMode::PreviousStart,
                    value: block.offset_seconds,
                    edit: None,
                    opened,
                },
            )
        });
    }
}
fn close(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<Draft>(key()));
    crate::app::native_menu::close(ctx, Id::new("timestamp-origin"));
}

pub(crate) fn show(app: &mut EditorApp, ui: &egui::Ui, ctx: &egui::Context, screen: Rect) {
    let Some(mut draft) = ctx.data(|d| d.get_temp::<Draft>(key())) else {
        return;
    };
    if app.popup.is_some() || !ui.is_enabled() {
        close(ctx);
        return;
    }
    let Some(block) = app.project.blocks.iter().find(|b| b.id == draft.block) else {
        close(ctx);
        return;
    };
    draft.original_mode = block.offset_mode;
    draft.original_value = block.offset_seconds;
    draft.original_start = block.timestamp;
    let size = Vec2::new(276.0, 341.0).min(screen.size());
    let below = draft.anchor.top() + ((draft.anchor.height() as i32 / 2) as f32)
        < screen.top() + (screen.height() as i32 / 2) as f32;
    let mut pos = Pos2::new(
        draft.anchor.left() + (draft.anchor.width() - 276.0) * 0.5,
        if below {
            draft.anchor.bottom() - 2.0
        } else {
            draft.anchor.top() - 341.0
        },
    );
    pos.x = pos.x.trunc().clamp(screen.left(), screen.right() - size.x);
    pos.y = pos.y.trunc().clamp(screen.top(), screen.bottom() - size.y);
    let outer = Rect::from_min_size(pos, size);
    let child_open = egui::Popup::is_id_open(ctx, Id::new("timestamp-origin"));
    let mut cancel = false;
    let mut accept = false;
    egui::Area::new(Id::new("timestamp-popup-host"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            ui.set_min_size(size);
            let slot = ui.painter().add(egui::Shape::Noop);
            crate::app::anchored_popup::paint_direction(ui, slot, outer, below);
            let host = ui.interact(
                outer.shrink(9.0),
                Id::new("timestamp-host-focus"),
                Sense::click(),
            );
            if ctx.cumulative_frame_nr() == draft.opened {
                host.request_focus();
            }
            let rect = |x, y, w, h| Rect::from_min_size(pos + Vec2::new(x, y), Vec2::new(w, h));
            let tr = |key: &'static str| i18n::text(app.locale, key);
            ui.painter().text(
                pos + Vec2::new(32.0, 46.5),
                egui::Align2::LEFT_CENTER,
                tr("Timestamp Settings"),
                style::font(23.04 / 1.48),
                style::TEXT,
            );
            let reset = ui.interact(
                rect(215.0, 32.0, 29.0, 29.0),
                Id::new("timestamp-reset"),
                Sense::click(),
            );
            crate::app::native_icons::paint_dialog(ui, reset.rect, 12);
            let reset = crate::app::root_tooltip::attach(reset, tr("Reset"));
            if reset.clicked() {
                draft.edit = None;
                draft.relative = true;
                draft.end = true;
                draft.value = 0.0;
            }
            ui.painter().text(
                pos + Vec2::new(32.0, 85.5),
                egui::Align2::LEFT_CENTER,
                tr("Count from"),
                style::font(20.880_001 / 1.48),
                style::TEXT,
            );
            let combo_first = tr("@@TIMESTAMP_PANEL_COUNT_FROM_BUTTOM_AT_BOTTOM@@") == "true";
            let (combo_y, choices_y) = if combo_first {
                (106.0, 150.0)
            } else {
                (146.0, 106.0)
            };
            if crate::app::dialog_controls::combo(
                ui,
                rect(32.0, combo_y, 212.0, 32.0),
                Id::new("timestamp-origin"),
                &mut draft.relative,
                &[
                    (true, tr("Previous Block")),
                    (false, tr("Beginning of Project")),
                ],
            ) {
                draft.edit = None;
                draft.value = if draft.relative {
                    if draft.original_mode == OffsetMode::Absolute {
                        0.0
                    } else {
                        draft.original_value
                    }
                } else if draft.original_mode == OffsetMode::Absolute {
                    draft.original_value.max(0.0)
                } else {
                    draft.original_start.max(0.0)
                };
            }
            if controls::segment(
                ui,
                rect(32.0, choices_y, 106.0, 28.0),
                Id::new("timestamp-start"),
                tr("Beginning Of"),
                !draft.end,
                draft.relative,
                draft.color,
                true,
            ) {
                draft.end = false;
            }
            if controls::segment(
                ui,
                rect(138.0, choices_y, 106.0, 28.0),
                Id::new("timestamp-end"),
                tr("End Of"),
                draft.end,
                draft.relative,
                draft.color,
                false,
            ) {
                draft.end = true;
            }
            ui.painter().text(
                pos + Vec2::new(32.0, 206.0),
                egui::Align2::LEFT_CENTER,
                tr("Offset"),
                style::font(20.880_001 / 1.48),
                style::TEXT,
            );
            controls::value(ui, rect(144.0, 190.0, 100.0, 32.0), &mut draft);
            accept = paint::button(
                ui,
                rect(32.0, 277.0, 102.0, 32.0),
                Id::new("timestamp-ok"),
                tr("OK"),
                draft.color,
                Color32::WHITE,
            )
            .clicked();
            cancel = paint::button(
                ui,
                rect(142.0, 277.0, 102.0, 32.0),
                Id::new("timestamp-cancel"),
                tr("Cancel"),
                Color32::WHITE,
                Color32::from_gray(78),
            )
            .clicked();
        });
    let outside = !child_open
        && ctx.cumulative_frame_nr() > draft.opened
        && ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !outer.shrink(9.0).contains(p) && !draft.anchor.contains(p))
        });
    if accept {
        if let Some(edit) = &draft.edit {
            let text = if edit.composing {
                &edit.stable
            } else {
                &edit.text
            };
            let text = controls::normalized(text);
            draft.value = number::parse(&text, draft.value, !draft.relative);
        }
        if draft.value.is_finite() {
            if let Some(index) = app.project.blocks.iter().position(|b| b.id == draft.block) {
                let mode = if !draft.relative {
                    OffsetMode::Absolute
                } else if draft.end {
                    OffsetMode::PreviousEnd
                } else {
                    OffsetMode::PreviousStart
                };
                if app.project.blocks[index].offset_mode != mode
                    || app.project.blocks[index].offset_seconds.to_bits() != draft.value.to_bits()
                {
                    app.begin_edit();
                    app.project.blocks[index].offset_mode = mode;
                    app.project.blocks[index].offset_seconds = draft.value;
                }
            }
        }
        close(ctx);
    } else if cancel || outside {
        close(ctx);
    } else {
        ctx.data_mut(|d| d.insert_temp(key(), draft));
    }
}
