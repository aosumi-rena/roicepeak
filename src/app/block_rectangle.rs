use crate::app::editor::{EditorApp, script_layout::Geometry};
use eframe::egui::{self, Pos2, Rect, Vec2};
use std::collections::BTreeSet;

#[derive(Clone)]
struct Gesture {
    origin: Pos2,
    control: bool,
    preview: BTreeSet<u64>,
}

fn id() -> egui::Id {
    egui::Id::new("block-selection-rectangle")
}

pub(super) fn cancel(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<Gesture>(id()));
}

fn nearest(geometry: &[Geometry], y: f32) -> Option<usize> {
    geometry
        .iter()
        .position(|item| item.card.bottom() >= y)
        .or_else(|| geometry.len().checked_sub(1))
}

pub(super) fn update(
    app: &mut EditorApp,
    ui: &egui::Ui,
    surface: &egui::Response,
    geometry: &[Geometry],
) -> BTreeSet<u64> {
    let mut gesture = ui.data_mut(|data| data.get_temp::<Gesture>(id()));
    let (pointer, pressed, released, modifiers) = ui.input(|input| {
        (
            input.pointer.interact_pos(),
            input.pointer.primary_pressed(),
            input.pointer.primary_released(),
            input.modifiers,
        )
    });
    if !ui.is_enabled() {
        cancel(ui.ctx());
        return BTreeSet::new();
    }
    if pressed && surface.is_pointer_button_down_on() {
        if let Some(pointer) = pointer {
            let origin = pointer + Vec2::new(0.0, app.scroll);
            app.selection.begin_rectangle(
                &app.project,
                nearest(geometry, origin.y),
                modifiers.shift,
                modifiers.ctrl,
            );
            surface.request_focus();
            gesture = Some(Gesture {
                origin,
                control: modifiers.ctrl,
                preview: BTreeSet::new(),
            });
        }
    }
    if let (Some(gesture), Some(pointer)) = (&mut gesture, pointer) {
        let point = pointer + Vec2::new(0.0, app.scroll);
        let rectangle = Rect::from_two_pos(gesture.origin, point);
        gesture.preview.clear();
        if rectangle.width() > 0.0 && rectangle.height() > 0.0 {
            gesture
                .preview
                .extend(geometry.iter().zip(&app.project.blocks).filter_map(
                    |(geometry, block)| {
                        let overlap = geometry.card.intersect(rectangle);
                        (overlap.width() > 0.0 && overlap.height() > 0.0).then_some(block.id)
                    },
                ));
        }
    }
    if released {
        if let (Some(gesture), Some(pointer)) = (gesture.take(), pointer) {
            let point = pointer + Vec2::new(0.0, app.scroll);
            let rectangle = Rect::from_two_pos(gesture.origin, point);
            if rectangle.width() > 0.0 && rectangle.height() > 0.0 {
                app.selection.finish_rectangle(
                    &app.project,
                    &gesture.preview,
                    gesture.control,
                    nearest(geometry, point.y),
                );
            } else if let Some(index) = geometry.iter().position(|item| {
                item.card.left() <= gesture.origin.x
                    && gesture.origin.x < item.card.right()
                    && item.card.top() <= gesture.origin.y
                    && gesture.origin.y < item.card.bottom()
            }) {
                app.click_block(index, modifiers.shift, modifiers.ctrl);
            }
            ui.ctx().request_repaint();
        }
    }
    let preview = gesture
        .as_ref()
        .map(|gesture| gesture.preview.clone())
        .unwrap_or_default();
    ui.data_mut(|data| {
        if let Some(gesture) = gesture {
            data.insert_temp(id(), gesture);
        } else {
            data.remove::<Gesture>(id());
        }
    });
    preview
}
