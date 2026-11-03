use eframe::egui::{self, Id, Pos2, Response, Ui};

#[derive(Clone, Copy)]
struct Capture {
    shape: u64,
    origin: Pos2,
}

pub(super) fn captured(ui: &Ui, id: Id, shape: u64) -> bool {
    ui.is_enabled()
        && crate::app::phoneme_history::allowed(ui.ctx())
        && crate::app::phoneme_history::load_capture::<Capture>(ui.ctx(), id)
            .is_some_and(|capture| capture.shape == shape)
        && ui.input(|input| {
            input.pointer.primary_down()
                || input.pointer.button_released(egui::PointerButton::Primary)
        })
}

pub(super) fn point(
    ui: &Ui,
    response: &Response,
    id: Id,
    shape: u64,
    enabled: bool,
    plot_top: f32,
    target: crate::app::phoneme_history::Target,
) -> Option<Pos2> {
    if !crate::app::phoneme_history::allowed(ui.ctx()) {
        crate::app::phoneme_history::clear_capture::<Capture>(ui.ctx(), id);
        return None;
    }
    let mut capture = crate::app::phoneme_history::load_capture::<Capture>(ui.ctx(), id)
        .filter(|capture| capture.shape == shape);
    let (down, released) = ui.input(|input| {
        (
            input.pointer.primary_down(),
            input.pointer.button_released(egui::PointerButton::Primary),
        )
    });
    let held = response.is_pointer_button_down_on() && down;
    let click = response.clicked_by(egui::PointerButton::Primary);
    let stopped = response.drag_stopped_by(egui::PointerButton::Primary);
    if enabled
        && ui.is_enabled()
        && capture.is_none()
        && crate::app::phoneme_history::pressed(ui.ctx())
        && (held || click || stopped)
    {
        let origin = ui.input(|input| {
            input.pointer.press_origin().or_else(|| {
                input.events.iter().rev().find_map(|event| match event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    } => Some(*pos),
                    _ => None,
                })
            })
        });
        if let Some(origin) = origin {
            capture = Some(Capture { shape, origin });
        }
    }
    let result = capture
        .filter(|capture| {
            enabled
                && ui.is_enabled()
                && capture.origin.y >= plot_top
                && (held || click || stopped || released)
        })
        .and_then(|_| {
            let release = released
                .then(|| {
                    ui.input(|input| {
                        input.events.iter().rev().find_map(|event| match event {
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed: false,
                                ..
                            } => Some(*pos),
                            _ => None,
                        })
                    })
                })
                .flatten();
            release
                .or_else(|| response.interact_pointer_pos())
                .or_else(|| ui.input(|input| input.pointer.interact_pos()))
        });
    if capture.is_some_and(|c| c.origin.y >= plot_top)
        && enabled
        && ui.is_enabled()
        && (held || click || stopped || released)
    {
        crate::app::phoneme_history::note(ui.ctx(), id, target, released || !down);
    }
    if released || !down || !enabled || !ui.is_enabled() {
        crate::app::phoneme_history::clear_capture::<Capture>(ui.ctx(), id);
    } else if let Some(capture) = capture {
        crate::app::phoneme_history::store_capture(ui.ctx(), id, capture);
    }
    result
}
