//! Windows portrait wheel events before framework smoothing or Ctrl-wheel zoom.
use eframe::egui::{self, Context, Id, LayerId, Pos2, RawInput, Rect, Ui};

#[derive(Clone)]
struct Region {
    identity: String,
    card: Rect,
    layer: LayerId,
    enabled: bool,
    frame: u64,
}

#[derive(Clone)]
pub(super) struct Step {
    pub delta_y: f32,
    pub pointer: Pos2,
}

#[derive(Clone, Default)]
struct Pending {
    frame: u64,
    identity: String,
    steps: Vec<Step>,
}

fn region_id(viewport: egui::ViewportId) -> Id {
    Id::new(("portrait-wheel-region", viewport))
}
fn pending_id(viewport: egui::ViewportId) -> Id {
    Id::new(("portrait-wheel-pending", viewport))
}

pub(super) fn register(ui: &Ui, card: Rect, identity: &str) {
    let region = Region {
        identity: identity.to_owned(),
        card: card.intersect(ui.clip_rect()),
        layer: ui.layer_id(),
        enabled: ui.is_enabled(),
        frame: ui.ctx().cumulative_frame_nr(),
    };
    let id = region_id(ui.ctx().viewport_id());
    ui.ctx().data_mut(|data| {
        data.insert_temp(id, region);
    });
}

pub(super) fn raw_input(ctx: &Context, input: &mut RawInput) {
    if !cfg!(target_os = "windows") || ctx.viewport_id() != input.viewport_id {
        return;
    }
    let Some(region) = ctx.data(|data| data.get_temp::<Region>(region_id(input.viewport_id)))
    else {
        return;
    };
    let frame = ctx.cumulative_frame_nr_for(input.viewport_id);
    if region.frame.checked_add(1) != Some(frame) || !region.enabled {
        return;
    }
    let mut pointer = ctx.input_for(input.viewport_id, |state| state.pointer.hover_pos());
    input.events.retain(|event| {
        match event {
            egui::Event::PointerMoved(position) => pointer = Some(*position),
            egui::Event::PointerButton { pos, .. } => pointer = Some(*pos),
            egui::Event::PointerGone => pointer = None,
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta,
                ..
            } if delta.is_finite() => {
                let Some(pointer) = pointer else {
                    return true;
                };
                if !region.card.contains(pointer) || ctx.layer_id_at(pointer) != Some(region.layer)
                {
                    return true;
                }
                let raw = (delta.y * 120.0)
                    .round_ties_even()
                    .clamp(f32::from(i16::MIN), f32::from(i16::MAX))
                    as i16;
                let delta_y = (f32::from(raw) * 0.5).clamp(-1000.0, 1000.0) * (1.0 / 256.0);
                ctx.data_mut(|data| {
                    let id = pending_id(input.viewport_id);
                    let mut pending = data
                        .get_temp::<Pending>(id)
                        .filter(|pending| {
                            pending.frame == frame && pending.identity == region.identity
                        })
                        .unwrap_or_else(|| Pending {
                            frame,
                            identity: region.identity.clone(),
                            steps: Vec::new(),
                        });
                    pending.steps.push(Step { delta_y, pointer });
                    data.insert_temp(id, pending);
                });
                return false;
            }
            _ => {}
        }
        true
    });
}

pub(super) fn take(ui: &Ui, card: Rect, identity: &str) -> Vec<Step> {
    let id = pending_id(ui.ctx().viewport_id());
    let pending = ui.ctx().data_mut(|data| data.remove_temp::<Pending>(id));
    if !ui.is_enabled() {
        return Vec::new();
    }
    pending
        .filter(|pending| {
            pending.frame == ui.ctx().cumulative_frame_nr() && pending.identity == identity
        })
        .map_or_else(Vec::new, |pending| {
            pending
                .steps
                .into_iter()
                .filter(|step| {
                    card.intersect(ui.clip_rect()).contains(step.pointer)
                        && ui.ctx().layer_id_at(step.pointer) == Some(ui.layer_id())
                })
                .collect()
        })
}

pub(super) fn unregister(ctx: &Context) {
    let viewport = ctx.viewport_id();
    let region = region_id(viewport);
    let pending = pending_id(viewport);
    ctx.data_mut(|data| {
        data.remove::<Region>(region);
        data.remove::<Pending>(pending);
    });
}
