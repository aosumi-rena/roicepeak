//! Shared character image menu.
use crate::app::editor::EditorApp;
use crate::app::style;
use eframe::egui::{self, Color32, Id, Rect, Sense, Stroke, Vec2};

struct Item {
    key: Option<String>,
    avatar: bool,
    label: String,
    separator: bool,
    width: f32,
}
impl Item {
    fn height(&self) -> f32 {
        if self.separator { 12.0 } else { 48.0 }
    }
}

#[derive(Clone, Default)]
struct Session {
    selected: Option<usize>,
    keyboard: bool,
    pointer: Option<egui::Pos2>,
    previous_focus: Option<Id>,
}
impl Session {
    fn step(&mut self, items: &[Item], backwards: bool) {
        self.keyboard = true;
        let count = items.len();
        let start = self
            .selected
            .unwrap_or(if backwards { 0 } else { count - 1 });
        for distance in 1..=count {
            let index = if backwards {
                (start + count - distance) % count
            } else {
                (start + distance) % count
            };
            if !items[index].separator {
                self.selected = Some(index);
                break;
            }
        }
    }
}

struct Layout {
    columns: Vec<f32>,
    rows_per_column: usize,
    height: f32,
    width: f32,
}
fn measure(items: &[Item], count: usize, available: f32, minimum: f32) -> Layout {
    let rows_per_column = items.len().div_ceil(count);
    let mut height = 0.0_f32;
    let mut columns = Vec::new();
    for index in 0..count {
        let start = (index * rows_per_column).min(items.len());
        let end = (start + rows_per_column).min(items.len());
        let column = &items[start..end];
        height = height.max(column.iter().map(Item::height).sum());
        let ideal = column
            .iter()
            .map(|item| item.width)
            .fold(48.0_f32, f32::max)
            + 16.0;
        columns.push(ideal.min((available / count.saturating_sub(2).max(1) as f32).trunc()));
    }
    let total: f32 = columns.iter().sum();
    let minimum = minimum.min(available);
    let width = if total < minimum {
        for width in &mut columns {
            *width = (minimum / count as f32).trunc();
        }
        minimum
    } else {
        total
    };
    Layout {
        columns,
        rows_per_column,
        height,
        width,
    }
}
fn layout(items: &[Item], available: Vec2, minimum: f32, single_column: bool) -> Layout {
    let mut count = 1;
    let mut plan = measure(items, count, available.x, minimum);
    while !single_column
        && plan.width <= available.x * 0.5
        && plan.height >= available.y
        && count < 7
    {
        count += 1;
        plan = measure(items, count, available.x, minimum);
    }
    if plan.width > available.x {
        count = count.saturating_sub(1).max(1);
        plan = measure(items, count, available.x, minimum);
    }
    plan.height += 16.0;
    plan
}

fn keys(ui: &egui::Ui) -> Vec<egui::Key> {
    ui.input_mut(|input| {
        let mut keys = Vec::new();
        input.events.retain(|event| {
            if let egui::Event::Key {
                key, pressed: true, ..
            } = event
                && matches!(
                    key,
                    egui::Key::ArrowUp
                        | egui::Key::ArrowDown
                        | egui::Key::ArrowLeft
                        | egui::Key::ArrowRight
                        | egui::Key::Enter
                        | egui::Key::Space
                        | egui::Key::Escape
                )
            {
                keys.push(*key);
                return false;
            }
            true
        });
        keys
    })
}

pub(crate) struct Options<'a> {
    pub original: Option<&'a str>,
    pub excluded_identity: Option<&'a str>,
    pub minimum_width: f32,
    pub single_column: bool,
}

pub(crate) struct Outcome {
    pub key: Option<String>,
    pub closed: bool,
}

pub(crate) fn show_at(
    app: &mut EditorApp,
    parent: &egui::Ui,
    id: Id,
    anchor: Rect,
    initial: bool,
    choices: &[String],
) -> Outcome {
    anchored(app, parent, id, anchor, initial, choices, false)
}

pub(crate) fn show_list_at(
    app: &mut EditorApp,
    parent: &egui::Ui,
    id: Id,
    anchor: Rect,
    initial: bool,
    choices: &[String],
) -> Outcome {
    anchored(app, parent, id, anchor, initial, choices, true)
}

fn anchored(
    app: &mut EditorApp,
    parent: &egui::Ui,
    id: Id,
    anchor: Rect,
    initial: bool,
    choices: &[String],
    single_column: bool,
) -> Outcome {
    let reset = initial || !egui::Popup::is_id_open(parent.ctx(), id);
    let popup = egui::Popup::new(id, parent.ctx().clone(), anchor, parent.layer_id())
        .kind(egui::PopupKind::Menu)
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .open_memory(reset.then_some(egui::SetOpenCommand::Bool(true)));
    show_popup(
        app,
        parent,
        popup,
        id,
        reset,
        choices,
        Options {
            original: None,
            excluded_identity: None,
            minimum_width: anchor.width(),
            single_column,
        },
    )
}

pub(crate) fn show_popup(
    app: &mut EditorApp,
    parent: &egui::Ui,
    popup: egui::Popup<'_>,
    id: Id,
    reset: bool,
    choices: &[String],
    options: Options<'_>,
) -> Outcome {
    let ctx = parent.ctx();
    let session_id = id.with("session");
    let mut session = if reset {
        Session {
            previous_focus: ctx.memory(|memory| memory.focused()),
            pointer: ctx.pointer_hover_pos(),
            ..Session::default()
        }
    } else {
        ctx.data_mut(|data| data.get_temp::<Session>(session_id))
            .unwrap_or_default()
    };
    let mut items = Vec::new();
    if let Some(original) = options.original {
        items.push(Item {
            key: Some(original.to_owned()),
            avatar: false,
            label: app
                .tr("\u{5909}\u{66f4}\u{3057}\u{306a}\u{3044}", "Do not change")
                .into(),
            separator: false,
            width: 0.0,
        });
    }
    let mut commercial = None;
    for key in choices {
        let Some(voice) = app
            .catalog
            .instance(key)
            .filter(|voice| options.excluded_identity != Some(voice.name.as_str()))
        else {
            continue;
        };
        if commercial != Some(voice.can_commercial) && !items.is_empty() {
            items.push(Item {
                key: None,
                avatar: false,
                label: String::new(),
                separator: true,
                width: 50.0,
            });
        }
        commercial = Some(voice.can_commercial);
        items.push(Item {
            key: Some(key.clone()),
            avatar: true,
            label: app.instance_name(key),
            separator: false,
            width: 0.0,
        });
    }
    if items.is_empty() {
        egui::Popup::close_id(ctx, id);
        return Outcome {
            key: None,
            closed: true,
        };
    }
    for item in &mut items {
        if !item.separator {
            item.width = parent
                .painter()
                .layout_no_wrap(item.label.clone(), style::font(23.04 / 1.48), style::TEXT)
                .size()
                .x
                .ceil()
                + 56.0;
        }
    }
    let available = (ctx.viewport_rect().size() - Vec2::splat(24.0)).max(Vec2::splat(1.0));
    let plan = layout(
        &items,
        available,
        options.minimum_width,
        options.single_column,
    );
    let mut chosen = None;
    let mut closed = false;
    let mut bounds = None;
    popup
        .width(plan.width)
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .frame(egui::Frame::NONE)
        .show(|ui| {
            let focus_id = id.with("input");
            ctx.memory_mut(|memory| memory.set_modal_layer(ui.layer_id()));
            let focus = ui.interact(
                Rect::from_min_size(
                    ui.next_widget_position(),
                    Vec2::new(plan.width, plan.height.min(available.y)),
                ),
                focus_id,
                Sense::focusable_noninteractive(),
            );
            if reset {
                focus.request_focus();
            }
            ctx.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    focus_id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
            for key in keys(ui) {
                match key {
                    egui::Key::ArrowDown => session.step(&items, false),
                    egui::Key::ArrowUp => session.step(&items, true),
                    egui::Key::Enter | egui::Key::Space => chosen = session.selected,
                    egui::Key::Escape => closed = true,
                    _ => (),
                }
            }
            if let Some(pointer) = ctx.pointer_hover_pos() {
                if session
                    .pointer
                    .is_some_and(|previous| previous.distance(pointer).trunc() > 2.0)
                {
                    session.keyboard = false;
                }
                if !session.keyboard {
                    session.pointer = Some(pointer);
                }
            }
            let shown = egui::ScrollArea::vertical()
                .id_salt(id.with("scroll"))
                .max_height(available.y)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let (_, outer) = ui.allocate_space(Vec2::new(plan.width, plan.height));
                    ui.painter().rect_filled(outer, 0, Color32::WHITE);
                    ui.painter().rect_stroke(
                        outer,
                        0,
                        Stroke::new(1.0, Color32::from_gray(178)),
                        egui::StrokeKind::Inside,
                    );
                    let mut x = outer.left();
                    let mut pointer_on_item = false;
                    for (column, &width) in plan.columns.iter().enumerate() {
                        let mut y = outer.top() + 8.0;
                        let start = column * plan.rows_per_column;
                        for (offset, item) in items
                            .iter()
                            .skip(start)
                            .take(plan.rows_per_column)
                            .enumerate()
                        {
                            let slot = start + offset;
                            let cell = Rect::from_min_size(
                                egui::pos2(x, y),
                                Vec2::new(width, item.height()),
                            );
                            y += item.height();
                            if !session.keyboard && ui.rect_contains_pointer(cell) {
                                session.selected = Some(slot);
                                pointer_on_item = true;
                            }
                            if item.separator {
                                ui.painter().rect_filled(
                                    Rect::from_min_size(
                                        cell.min + Vec2::new(0.0, 6.0),
                                        Vec2::new(width, 1.0),
                                    ),
                                    0,
                                    Color32::from_gray(225),
                                );
                                continue;
                            }
                            let response = ui.interact(cell, id.with(slot), Sense::click());
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    &item.label,
                                )
                            });
                            if session.selected == Some(slot) {
                                ui.painter().rect_filled(
                                    cell.shrink(1.0),
                                    0,
                                    Color32::from_gray(242),
                                );
                                if session.keyboard {
                                    ui.scroll_to_rect(cell, None);
                                }
                            }
                            let left = if let Some(key) = item.key.as_ref().filter(|_| item.avatar)
                            {
                                app.paint_instance_avatar(
                                    ui,
                                    key,
                                    Rect::from_min_size(
                                        cell.min + Vec2::new(12.0, 8.0),
                                        Vec2::splat(32.0),
                                    ),
                                );
                                56.0
                            } else {
                                38.0
                            };
                            let label = Rect::from_min_max(
                                cell.min + Vec2::new(left, 1.0),
                                egui::pos2(
                                    (cell.right() - 30.0).max(cell.left() + left),
                                    cell.bottom() - 1.0,
                                ),
                            );
                            let color = item
                                .key
                                .as_ref()
                                .filter(|_| item.avatar)
                                .map_or(style::TEXT, |key| app.instance_color(key));
                            fitted_name(ui, label, &item.label, color, 0.7);
                            if response.clicked() {
                                chosen = Some(slot);
                            }
                        }
                        x += width;
                    }
                    if !session.keyboard && ui.rect_contains_pointer(outer) && !pointer_on_item {
                        session.selected = None;
                    }
                });
            bounds = Some(shown.inner_rect);
        });
    let key = chosen
        .filter(|&slot| items.get(slot).is_some_and(|item| !item.separator))
        .and_then(|slot| items[slot].key.clone());
    if key.is_some() {
        closed = true;
    }
    if !reset
        && ctx.input(|input| input.pointer.any_click())
        && ctx
            .pointer_interact_pos()
            .is_some_and(|pointer| bounds.is_some_and(|bounds| !bounds.contains(pointer)))
    {
        closed = true;
    }
    closed |= !egui::Popup::is_id_open(ctx, id);
    if closed {
        egui::Popup::close_id(ctx, id);
        ctx.memory_mut(|memory| {
            if memory.has_focus(id.with("input")) {
                if let Some(previous) = session.previous_focus {
                    memory.request_focus(previous);
                } else {
                    memory.surrender_focus(id.with("input"));
                }
            }
        });
    }
    ctx.data_mut(|data| data.insert_temp(session_id, session));
    Outcome { key, closed }
}

fn fitted_name(ui: &egui::Ui, rect: Rect, name: &str, color: Color32, min_scale: f32) {
    let mut galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), style::font(23.04 / 1.48), color);
    let scale = (rect.width() / galley.size().x.max(1.0)).clamp(min_scale, 1.0);
    if scale < 1.0 {
        let galley = std::sync::Arc::make_mut(&mut galley);
        for row in &mut galley.rows {
            row.pos.x *= scale;
            let row = std::sync::Arc::make_mut(&mut row.row);
            for vertex in &mut row.visuals.mesh.vertices {
                vertex.pos.x *= scale;
            }
            row.visuals.mesh_bounds.min.x *= scale;
            row.visuals.mesh_bounds.max.x *= scale;
        }
        galley.rect.min.x *= scale;
        galley.rect.max.x *= scale;
        galley.mesh_bounds.min.x *= scale;
        galley.mesh_bounds.max.x *= scale;
    }
    let at = egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5);
    ui.painter()
        .with_clip_rect(rect.intersect(ui.clip_rect()))
        .galley(at, galley, color);
}
