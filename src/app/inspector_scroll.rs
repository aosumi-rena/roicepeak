use eframe::egui::{self, Align, Layout, Rect, Sense, Ui, UiBuilder, Vec2};

const SCROLL_SALT: &str = "inspector-scroll";

#[derive(Clone, Copy)]
pub(crate) enum WheelRoute {
    Vertical,
    Horizontal { bar: Option<Rect> },
}

#[derive(Clone)]
struct WheelRegion {
    id: egui::Id,
    rect: Rect,
    layer: egui::LayerId,
    enabled: bool,
    route: WheelRoute,
}

#[derive(Clone, Default)]
struct WheelRegions {
    frame: u64,
    regions: Vec<WheelRegion>,
}

#[derive(Clone, Default)]
struct PendingWheel {
    frame: u64,
    steps: Vec<WheelStep>,
}

#[derive(Clone)]
struct WheelStep {
    amount: f64,
    pointer: egui::Pos2,
}

fn wheel_regions_id(viewport: egui::ViewportId) -> egui::Id {
    egui::Id::new(("native-scroll-regions", viewport))
}

/// Route Windows line events before egui adds its wheel smoothing.
pub(crate) fn raw_input(ctx: &egui::Context, input: &mut egui::RawInput) {
    if !cfg!(target_os = "windows") || ctx.viewport_id() != input.viewport_id {
        return;
    }
    let id = wheel_regions_id(input.viewport_id);
    let Some(regions) = ctx.data(|data| data.get_temp::<WheelRegions>(id)) else {
        return;
    };
    let frame = ctx.cumulative_frame_nr_for(input.viewport_id);
    if regions.frame.checked_add(1) != Some(frame) {
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
                let layer = ctx.layer_id_at(pointer);
                let Some(region) = regions.regions.iter().rev().find(|region| {
                    region.enabled && region.rect.contains(pointer) && layer == Some(region.layer)
                }) else {
                    return true;
                };
                let (delta, viewport) = match region.route {
                    WheelRoute::Vertical => (delta.y, false),
                    WheelRoute::Horizontal { bar }
                        if bar.is_some_and(|bar| bar.contains(pointer)) =>
                    {
                        (delta.x, false)
                    }
                    WheelRoute::Horizontal { .. } => (delta.y, true),
                };
                let raw = (delta * 120.0)
                    .round_ties_even()
                    .clamp(f32::from(i16::MIN), f32::from(i16::MAX))
                    as i16;
                let amount = (f32::from(raw) * 0.5).clamp(-1000.0, 1000.0);
                let amount = amount * (1.0 / 256.0);
                let amount = if viewport {
                    f64::from(amount) * 16.0 * 12.0
                } else {
                    let scaled = amount * 10.0;
                    let scaled = if scaled < 0.0 {
                        scaled.min(-1.0)
                    } else if scaled > 0.0 {
                        scaled.max(1.0)
                    } else {
                        0.0
                    };
                    f64::from(scaled) * 16.0
                };
                let pending_id = region.id.with("native-wheel");
                ctx.data_mut(|data| {
                    let mut pending = data
                        .get_temp::<PendingWheel>(pending_id)
                        .filter(|pending| pending.frame == frame)
                        .unwrap_or_else(|| PendingWheel {
                            frame,
                            steps: Vec::new(),
                        });
                    pending.steps.push(WheelStep { amount, pointer });
                    data.insert_temp(pending_id, pending);
                });
                return false;
            }
            _ => {}
        }
        true
    });
}

pub(crate) fn register_wheel_region(
    ui: &Ui,
    id: egui::Id,
    rect: Rect,
    enabled: bool,
    route: WheelRoute,
) {
    let viewport = ui.ctx().viewport_id();
    let frame = ui.ctx().cumulative_frame_nr();
    let region = WheelRegion {
        id,
        rect: rect.intersect(ui.clip_rect()),
        layer: ui.layer_id(),
        enabled: enabled && ui.is_enabled(),
        route,
    };
    ui.data_mut(|data| {
        let regions_id = wheel_regions_id(viewport);
        let mut regions = data
            .get_temp::<WheelRegions>(regions_id)
            .filter(|regions| regions.frame == frame)
            .unwrap_or_else(|| WheelRegions {
                frame,
                regions: Vec::new(),
            });
        regions.regions.retain(|existing| existing.id != id);
        regions.regions.push(region);
        data.insert_temp(regions_id, regions);
    });
}

pub(crate) fn wheel_offset(
    ui: &Ui,
    id: egui::Id,
    viewport: Rect,
    enabled: bool,
    mut offset: f64,
    maximum: f64,
) -> f64 {
    let wheel_id = id.with("native-wheel");
    let frame = ui.ctx().cumulative_frame_nr();
    let pending = ui.data_mut(|data| {
        let pending = data.get_temp::<PendingWheel>(wheel_id);
        data.remove::<PendingWheel>(wheel_id);
        pending
    });
    if enabled && ui.is_enabled() {
        if let Some(pending) = pending.filter(|pending| pending.frame == frame) {
            for step in pending.steps {
                if viewport.intersect(ui.clip_rect()).contains(step.pointer)
                    && ui.ctx().layer_id_at(step.pointer) == Some(ui.layer_id())
                {
                    offset = (offset - step.amount).clamp(0.0, maximum);
                }
            }
        }
    }
    offset
}

#[derive(Clone, Copy)]
enum Held {
    Thumb { pointer: f64, offset: f64 },
    Track { repeat_at: f64 },
}

struct Thumb {
    start: f64,
    length: f64,
    travel: f64,
    draggable: bool,
}

impl Thumb {
    fn new(track: Rect, extent: f64, page: f64, offset: f64) -> Self {
        let height = f64::from(track.height().round());
        let minimum = 2.0 * 16.0_f64.min(height);
        let short = height < minimum + 32.0;
        let available = if short { 0.0 } else { height };
        let mut length = (available * page / extent).round_ties_even();
        if length < minimum {
            length = minimum.min(available - 1.0);
        }
        length = length.min(available);
        let travel = (available - length).max(0.0);
        let start = if short { (height / 2.0).trunc() } else { 0.0 }
            + (travel * offset / (extent - page)).round_ties_even();
        Self {
            start,
            length,
            travel,
            draggable: available > minimum && available > length,
        }
    }
}

pub(crate) fn show<R>(
    ui: &mut Ui,
    viewport: Rect,
    card_size: Vec2,
    add_contents: impl FnOnce(&mut Ui, Rect) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    show_inner(
        ui,
        viewport,
        Vec2::new(viewport.width(), card_size.y + 48.0),
        Vec2::new(8.0, 21.0),
        true,
        false,
        |ui, content| {
            let card = Rect::from_min_size(content.min + Vec2::splat(24.0), card_size);
            add_contents(ui, card)
        },
    )
}

pub(crate) fn show_content<R>(
    ui: &mut Ui,
    viewport: Rect,
    content_size: Vec2,
    bar_insets: Vec2,
    reset: bool,
    add_contents: impl FnOnce(&mut Ui, Rect) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    show_inner(
        ui,
        viewport,
        content_size,
        bar_insets,
        true,
        reset,
        add_contents,
    )
}

fn show_inner<R>(
    ui: &mut Ui,
    viewport: Rect,
    content_size: Vec2,
    bar_insets: Vec2,
    truncate_offset: bool,
    reset: bool,
    add_contents: impl FnOnce(&mut Ui, Rect) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    let content_style = ui.style().clone();
    let mut host = ui.new_child(
        UiBuilder::new()
            .max_rect(viewport)
            .layout(Layout::top_down(Align::Min)),
    );
    host.set_clip_rect(viewport.intersect(ui.clip_rect()));
    // ScrollArea hashes its input salt before deriving the persistent state ID.
    let scroll_id = host.make_persistent_id(egui::IdSalt::new(SCROLL_SALT));
    let held_id = scroll_id.with("native-thumb");
    if reset {
        egui::scroll_area::State::default().store(host.ctx(), scroll_id);
        host.data_mut(|data| data.remove::<Held>(held_id));
    }
    let page = f64::from(viewport.height().max(0.0));
    let extent = f64::from(content_size.y.max(0.0));
    let maximum = (extent - page).max(0.0);
    let mut offset = f64::from(
        egui::scroll_area::State::load(host.ctx(), scroll_id)
            .unwrap_or_default()
            .offset
            .y,
    )
    .clamp(0.0, maximum);
    let track = Rect::from_min_size(
        egui::pos2(
            viewport.right() - bar_insets.x - 16.0,
            viewport.top() + bar_insets.y,
        ),
        Vec2::new(16.0, (viewport.height() - bar_insets.y).max(0.0)),
    );
    let visible = extent > page && page > 0.0;
    register_wheel_region(&host, scroll_id, viewport, visible, WheelRoute::Vertical);
    offset = wheel_offset(&host, scroll_id, viewport, visible, offset, maximum);
    let mut held = host.data_mut(|data| data.get_temp::<Held>(held_id));
    let (pointer, pressed, down, now) = host.input(|input| {
        (
            input.pointer.interact_pos(),
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.time,
        )
    });
    let mut hover = false;
    if visible && host.is_enabled() {
        let response = host.interact(track, held_id, Sense::DRAG);
        hover = response.hovered();
        let thumb = Thumb::new(track, extent, page, offset);
        if let Some(pointer) = pointer {
            let y = f64::from((pointer.y - track.top()).round());
            if pressed && host.rect_contains_pointer(track) {
                if y < thumb.start {
                    offset = (offset - page).max(0.0);
                    held = Some(Held::Track {
                        repeat_at: now + 0.4,
                    });
                } else if y >= thumb.start + thumb.length {
                    offset = (offset + page).min(maximum);
                    held = Some(Held::Track {
                        repeat_at: now + 0.4,
                    });
                } else if thumb.draggable {
                    held = Some(Held::Thumb { pointer: y, offset });
                }
            } else if down {
                match held {
                    Some(Held::Thumb {
                        pointer: origin,
                        offset: initial,
                    }) if thumb.draggable => {
                        offset =
                            (initial + (y - origin) * maximum / thumb.travel).clamp(0.0, maximum);
                    }
                    Some(Held::Track { repeat_at }) if now >= repeat_at => {
                        if y < thumb.start {
                            offset = (offset - page).max(0.0);
                        } else if y > thumb.start + thumb.length {
                            offset = (offset + page).min(maximum);
                        }
                        held = Some(Held::Track {
                            repeat_at: now + 0.04,
                        });
                    }
                    _ => {}
                }
            }
        }
    }
    if !down || !visible || !host.is_enabled() {
        held = None;
    }
    host.data_mut(|data| {
        if let Some(held) = held {
            data.insert_temp(held_id, held);
        } else {
            data.remove::<Held>(held_id);
        }
    });
    if let Some(Held::Track { repeat_at }) = held {
        host.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(
                (repeat_at - now).max(0.0),
            ));
    }
    host.spacing_mut().scroll = egui::style::ScrollStyle {
        floating: true,
        floating_allocated_width: 0.0,
        bar_inner_margin: 0.0,
        bar_outer_margin: 13.0,
        bar_width: 6.0,
        floating_width: 6.0,
        dormant_handle_opacity: 1.0,
        active_handle_opacity: 1.0,
        interact_handle_opacity: 1.0,
        dormant_background_opacity: 0.0,
        active_background_opacity: 0.0,
        interact_background_opacity: 0.0,
        fade: egui::style::ScrollFadeStyle {
            strength: 0.0,
            size: 0.0,
        },
        ..Default::default()
    };
    let output = egui::ScrollArea::vertical()
        .id_salt(SCROLL_SALT)
        .vertical_scroll_offset(offset as f32)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .auto_shrink([false, false])
        .max_width(viewport.width())
        .max_height(viewport.height())
        .min_scrolled_width(0.0)
        .min_scrolled_height(0.0)
        .scroll_bar_rect(Rect::from_min_max(
            viewport.min + Vec2::new(0.0, bar_insets.y.min(viewport.height())),
            viewport.max,
        ))
        .show_viewport(&mut host, |ui, visible_content| {
            ui.set_style(content_style);
            let (mut content, _) = ui.allocate_exact_size(content_size, Sense::hover());
            if truncate_offset {
                content = content.translate(Vec2::new(0.0, visible_content.top().fract()));
            }
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(content)
                    .layout(Layout::top_down(Align::Min)),
            );
            add_contents(&mut child, content)
        });
    if visible {
        let thumb = Thumb::new(track, extent, page, f64::from(output.state.offset.y));
        let body = Rect::from_min_size(
            egui::pos2(track.left() + 6.0, track.top() + thumb.start as f32 + 3.0),
            Vec2::new(4.0, (thumb.length as f32 - 6.0).max(0.0)),
        );
        host.painter().rect_filled(
            body,
            2,
            egui::Color32::from_gray(if hover || held.is_some() { 148 } else { 178 }),
        );
    }
    output
}
