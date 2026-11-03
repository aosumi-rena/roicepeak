use eframe::egui::{self, Color32, Rect, Sense, Ui, UiBuilder, Vec2};

const THICKNESS: f32 = 16.0;

#[derive(Clone, Copy)]
enum Held {
    Thumb { pointer: f64, offset: f64 },
    Track { repeat_at: f64 },
}

#[derive(Clone)]
struct State {
    held: Option<Held>,
    reset_pending: bool,
    last_frame: Option<u64>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            held: None,
            reset_pending: true,
            last_frame: None,
        }
    }
}

struct Thumb {
    start: f64,
    length: f64,
    travel: f64,
    draggable: bool,
}

impl Thumb {
    fn new(length: f64, extent: f64, page: f64, offset: f64) -> Self {
        let minimum = 2.0 * f64::from(THICKNESS).min(length);
        let short = length < minimum + 32.0;
        let available = if short { 0.0 } else { length };
        let mut size = (available * page / extent).round_ties_even();
        if size < minimum {
            size = minimum.min(available - 1.0);
        }
        size = size.min(available);
        let travel = (available - size).max(0.0);
        Self {
            start: if short { (length * 0.5).trunc() } else { 0.0 }
                + (travel * offset / (extent - page)).round_ties_even(),
            length: size,
            travel,
            draggable: available > minimum && available > size,
        }
    }
}

impl State {
    fn show<R>(
        &mut self,
        ui: &mut Ui,
        viewport: Rect,
        width: f32,
        add_contents: impl FnOnce(&mut Ui, Rect) -> R,
    ) -> R {
        let frame = ui.ctx().cumulative_frame_nr();
        if self.last_frame != Some(frame)
            && self.last_frame.and_then(|previous| previous.checked_add(1)) != Some(frame)
        {
            self.held = None;
        }
        self.last_frame = Some(frame);
        let visible = width > viewport.width();
        let area = Rect::from_min_size(
            viewport.min,
            Vec2::new(
                viewport.width(),
                (viewport.height() - if visible { THICKNESS } else { 0.0 }).max(0.0),
            ),
        );
        let track = Rect::from_min_size(
            egui::pos2(viewport.left(), area.bottom()),
            Vec2::new(viewport.width(), THICKNESS),
        );
        let mut host = ui.new_child(UiBuilder::new().max_rect(area));
        host.set_clip_rect(area.intersect(ui.clip_rect()));
        let salt = "phoneme-scroll";
        let scroll_id = host.make_persistent_id(egui::IdSalt::new(salt));
        if self.reset_pending {
            egui::scroll_area::State::default().store(host.ctx(), scroll_id);
            self.held = None;
            self.reset_pending = false;
        }
        let extent = f64::from(width);
        let page = f64::from(viewport.width());
        let maximum = (extent - page).max(0.0);
        let mut offset = f64::from(
            egui::scroll_area::State::load(host.ctx(), scroll_id)
                .unwrap_or_default()
                .offset
                .x,
        )
        .clamp(0.0, maximum);
        crate::app::inspector_scroll::register_wheel_region(
            ui,
            scroll_id,
            viewport,
            true,
            crate::app::inspector_scroll::WheelRoute::Horizontal {
                bar: visible.then_some(track),
            },
        );
        offset = crate::app::inspector_scroll::wheel_offset(
            ui, scroll_id, viewport, true, offset, maximum,
        );
        let (position, pressed, down, now) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.time,
            )
        });
        let mut hover = false;
        if visible && ui.is_enabled() {
            let response = ui.interact(track, ui.id().with("phoneme-bar"), Sense::DRAG);
            hover = response.hovered();
            let thumb = Thumb::new(page.round(), extent, page, offset);
            if let Some(position) = position {
                let x = f64::from((position.x - track.left()).round());
                if pressed && response.is_pointer_button_down_on() {
                    if x < thumb.start {
                        offset = (offset - page).max(0.0);
                        self.held = Some(Held::Track {
                            repeat_at: now + 0.4,
                        });
                    } else if x >= thumb.start + thumb.length {
                        offset = (offset + page).min(maximum);
                        self.held = Some(Held::Track {
                            repeat_at: now + 0.4,
                        });
                    } else if thumb.draggable {
                        self.held = Some(Held::Thumb { pointer: x, offset });
                    }
                } else if down {
                    match self.held {
                        Some(Held::Thumb {
                            pointer,
                            offset: initial,
                        }) if thumb.draggable => {
                            offset = (initial + (x - pointer) * maximum / thumb.travel)
                                .clamp(0.0, maximum);
                        }
                        Some(Held::Track { repeat_at }) if now >= repeat_at => {
                            if x < thumb.start {
                                offset = (offset - page).max(0.0);
                            } else if x > thumb.start + thumb.length {
                                offset = (offset + page).min(maximum);
                            }
                            self.held = Some(Held::Track {
                                repeat_at: now + 0.04,
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
        if !down || !visible || !ui.is_enabled() {
            self.held = None;
        }
        if let Some(Held::Track { repeat_at }) = self.held {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(
                    (repeat_at - now).max(0.0),
                ));
        }
        let output = egui::ScrollArea::horizontal()
            .id_salt(salt)
            .horizontal_scroll_offset(offset as f32)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .scroll_source(if cfg!(target_os = "windows") {
                egui::scroll_area::ScrollSource::NONE
            } else {
                egui::scroll_area::ScrollSource::MOUSE_WHEEL
            })
            .auto_shrink([false, false])
            .max_width(area.width())
            .max_height(area.height())
            .min_scrolled_width(0.0)
            .min_scrolled_height(0.0)
            .show_viewport(&mut host, |ui, _| {
                let (_, content) = ui.allocate_space(Vec2::new(width, 158.0));
                let offset = area.left() - content.left();
                let content = content.translate(Vec2::new(offset - offset.trunc(), 0.0));
                let mut child = ui.new_child(UiBuilder::new().max_rect(content));
                child.set_clip_rect(area.intersect(ui.clip_rect()));
                add_contents(&mut child, content)
            });
        if visible {
            let thumb = Thumb::new(page.round(), extent, page, f64::from(output.state.offset.x));
            if thumb.length > 0.0 {
                let body = Rect::from_min_size(
                    egui::pos2(track.left() + thumb.start as f32 + 3.0, track.top() + 7.0),
                    Vec2::new((thumb.length as f32 - 6.0).max(0.0), 4.0),
                );
                ui.painter()
                    .with_clip_rect(viewport.intersect(ui.clip_rect()))
                    .rect_filled(
                        body,
                        2,
                        Color32::from_gray(if hover || self.held.is_some() {
                            148
                        } else {
                            178
                        }),
                    );
            }
        }
        output.inner
    }
}

pub(super) fn show<R>(
    ui: &mut Ui,
    viewport: Rect,
    width: f32,
    add_contents: impl FnOnce(&mut Ui, Rect) -> R,
) -> R {
    let id = ui.id().with("phoneme-scroll-controls");
    let mut state = ui
        .data_mut(|data| data.get_temp::<State>(id))
        .unwrap_or_default();
    let result = state.show(ui, viewport, width, add_contents);
    ui.data_mut(|data| data.insert_temp(id, state));
    result
}
