use crate::app::style;
use eframe::egui::{self, Align2, Color32, Id, Rect, Response, Sense, Stroke, Vec2};

const FONT_SIZE: f32 = 23.040_001 * 1_000.0 / 1_480.0;

pub(crate) enum Item<'a, A> {
    Action {
        text: &'a str,
        shortcut: String,
        enabled: bool,
        action: A,
    },
    CheckedAction {
        text: &'a str,
        shortcut: String,
        enabled: bool,
        checked: bool,
        action: A,
    },
    Submenu {
        text: &'a str,
        enabled: bool,
        items: Vec<Self>,
    },
    Header {
        text: &'a str,
    },
    Separator,
}

impl<A: Copy> Item<'_, A> {
    fn text(&self) -> &str {
        match self {
            Self::Action { text, .. }
            | Self::CheckedAction { text, .. }
            | Self::Submenu { text, .. }
            | Self::Header { text } => text,
            Self::Separator => "",
        }
    }
    fn shortcut(&self) -> &str {
        match self {
            Self::Action { shortcut, .. } | Self::CheckedAction { shortcut, .. } => shortcut,
            _ => "",
        }
    }
    fn eligible(&self) -> bool {
        match self {
            Self::Action { enabled, .. } | Self::CheckedAction { enabled, .. } => *enabled,
            Self::Submenu { enabled, items, .. } => *enabled && !items.is_empty(),
            Self::Separator | Self::Header { .. } => false,
        }
    }
    fn children(&self) -> Option<&[Self]> {
        match self {
            Self::Submenu { items, .. } if self.eligible() => Some(items),
            _ => None,
        }
    }
    fn action(&self) -> Option<A> {
        match self {
            Self::Action {
                enabled: true,
                action,
                ..
            }
            | Self::CheckedAction {
                enabled: true,
                action,
                ..
            } => Some(*action),
            _ => None,
        }
    }
    fn height(&self) -> f32 {
        if matches!(self, Self::Separator) {
            12.0
        } else {
            33.0
        }
    }
}

#[derive(Clone, Default)]
struct Selection {
    slot: Option<usize>,
    keyboard: bool,
    initial: bool,
    pointer: Option<[i32; 2]>,
    changed: f64,
}

impl Selection {
    fn step<A: Copy>(&mut self, rows: &[Item<'_, A>], backwards: bool, now: f64) {
        self.keyboard = true;
        self.initial = false;
        if rows.is_empty() {
            return;
        }
        let count = rows.len();
        let start = self.slot.unwrap_or(if backwards { 0 } else { count - 1 });
        for distance in 1..=count {
            let slot = if backwards {
                (start + count - distance) % count
            } else {
                (start + distance) % count
            };
            if rows[slot].eligible() {
                self.select(Some(slot), now);
                return;
            }
        }
    }
    fn select(&mut self, slot: Option<usize>, now: f64) {
        if self.slot != slot {
            self.slot = slot;
            self.changed = now;
        }
    }
}

#[derive(Clone, Default)]
struct Session {
    levels: Vec<Selection>,
    parents: Vec<usize>,
    previous_focus: Option<Id>,
}

fn level_items<'a, 'b, A: Copy>(
    mut items: &'a [Item<'b, A>],
    parents: &[usize],
) -> Option<&'a [Item<'b, A>]> {
    for &index in parents {
        items = items.get(index)?.children()?;
    }
    Some(items)
}

fn pointer_position(ctx: &egui::Context) -> Option<[i32; 2]> {
    ctx.pointer_hover_pos()
        .map(|p| [p.x.round() as i32, p.y.round() as i32])
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

fn size<A: Copy>(ui: &egui::Ui, items: &[Item<'_, A>], minimum_width: f32) -> Vec2 {
    let font = style::font(FONT_SIZE);
    let width = items
        .iter()
        .filter(|item| !matches!(item, Item::Separator))
        .map(|item| {
            let text = if item.shortcut().is_empty() {
                item.text().to_owned()
            } else {
                format!("{}   {}", item.text(), item.shortcut())
            };
            let width = ui
                .painter()
                .layout_no_wrap(text, font.clone(), Color32::from_gray(102))
                .size()
                .x
                .ceil();
            if matches!(item, Item::Header { .. }) {
                let width = width + 56.0;
                width + (width / 4.0).trunc() + 16.0
            } else {
                width + 72.0
            }
        })
        .fold(72.0_f32, f32::max);
    Vec2::new(
        width
            .max(66.0)
            .max(minimum_width)
            .min((ui.ctx().viewport_rect().width() - 24.0).max(1.0)),
        16.0 + items.iter().map(Item::height).sum::<f32>(),
    )
}

struct Painted<A> {
    bounds: Rect,
    action: Option<A>,
    inside: bool,
}

fn paint<A: Copy>(
    ui: &mut egui::Ui,
    id: Id,
    items: &[Item<'_, A>],
    selection: &mut Selection,
    has_child: bool,
    now: f64,
    minimum_width: f32,
) -> Painted<A> {
    let size = size(ui, items, minimum_width);
    let (bounds, _) = ui.allocate_exact_size(size, Sense::hover());
    let mut rects = Vec::with_capacity(items.len());
    let mut y = bounds.top() + 8.0;
    for item in items {
        rects.push(Rect::from_min_size(
            egui::pos2(bounds.left(), y),
            Vec2::new(size.x, item.height()),
        ));
        y += item.height();
    }
    let inside = ui.rect_contains_pointer(bounds);
    if let Some(pointer) = ui.input(|input| input.pointer.hover_pos()) {
        let position = [pointer.x.round() as i32, pointer.y.round() as i32];
        if selection.keyboard
            && inside
            && selection.pointer.is_some_and(|previous| {
                (f64::from(previous[0]) - f64::from(position[0]))
                    .hypot(f64::from(previous[1]) - f64::from(position[1]))
                    .trunc()
                    > 2.0
            })
        {
            selection.keyboard = false;
        }
        if selection.initial && selection.pointer != Some(position) {
            selection.initial = false;
        }
        if !selection.keyboard && !selection.initial {
            selection.pointer = Some(position);
            if inside {
                selection.select(rects.iter().position(|rect| rect.contains(pointer)), now);
            } else if !has_child {
                selection.select(None, now);
            }
        }
    }
    ui.painter().rect_filled(bounds, 0, Color32::WHITE);
    ui.painter().rect_stroke(
        bounds,
        0,
        Stroke::new(1.0, Color32::from_gray(178)),
        egui::StrokeKind::Inside,
    );
    let mut action = None;
    let font = style::font(FONT_SIZE);
    for (index, (item, &rect)) in items.iter().zip(&rects).enumerate() {
        if matches!(item, Item::Separator) {
            ui.painter().rect_filled(
                Rect::from_min_size(
                    egui::pos2(rect.left(), rect.top() + 6.0),
                    Vec2::new(rect.width(), 1.0),
                ),
                0,
                Color32::from_gray(225),
            );
            continue;
        }
        if let Item::Header { text } = item {
            let label = Rect::from_min_size(
                rect.min + Vec2::new(21.0, -1.0),
                Vec2::new((rect.width() - 21.0).max(0.0), rect.height()),
            );
            ui.painter()
                .with_clip_rect(label.intersect(ui.clip_rect()))
                .text(
                    label.left_center(),
                    Align2::LEFT_CENTER,
                    text,
                    style::font(20.0 / 1.48),
                    Color32::from_gray(102),
                );
            continue;
        }
        let response = ui.interact(
            rect,
            id.with(index),
            if item.eligible() {
                Sense::CLICK
            } else {
                Sense::hover()
            },
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, item.eligible(), item.text())
        });
        if item.eligible() && selection.slot == Some(index) {
            ui.painter()
                .rect_filled(rect.shrink(1.0), 0, Color32::from_gray(242));
        }
        let mut label = Rect::from_min_max(
            egui::pos2(rect.left() + 30.0, rect.top() + 1.0),
            egui::pos2(
                (rect.right() - 30.0).max(rect.left() + 30.0),
                rect.bottom() - 1.0,
            ),
        );
        let arrow = matches!(item, Item::Submenu { .. }).then(|| {
            let height = FONT_SIZE * 1.16 * 0.6;
            let x = label.right() - height.trunc();
            label.set_right(x.max(label.left()));
            (height, x, label.center().y)
        });
        let painter = ui.painter().with_clip_rect(label.intersect(ui.clip_rect()));
        let color =
            Color32::from_rgba_unmultiplied(102, 102, 102, if item.eligible() { 255 } else { 128 });
        if matches!(item, Item::CheckedAction { checked: true, .. }) {
            let height = (rect.height() - 2.0).max(0.0);
            let margin = ((rect.width() as i32) / 20).min(5) as f32;
            let width = (height / 1.3)
                .round()
                .min((rect.width() - 2.0 - 2.0 * margin).max(0.0));
            let x = width.max(0.0) * 0.5 + rect.left() + 1.0 + margin - 2.0 - 11.0;
            let offset = (f64::from(height) / -6.0) as f32;
            let y = (height + offset + offset).max(0.0) * 0.5 + rect.top() + 1.0 - offset - 8.0;
            let middle = egui::pos2(x + 8.8, y + 12.8);
            ui.painter().line_segment(
                [egui::pos2(x + 4.4, y + 8.0), middle],
                Stroke::new(2.0, color),
            );
            ui.painter().line_segment(
                [middle, egui::pos2(x + 17.6, y + 3.2)],
                Stroke::new(2.0, color),
            );
        }
        painter.text(
            label.left_center(),
            Align2::LEFT_CENTER,
            item.text(),
            font.clone(),
            color,
        );
        if !item.shortcut().is_empty() {
            painter.text(
                label.right_center(),
                Align2::RIGHT_CENTER,
                item.shortcut(),
                font.clone(),
                Color32::from_gray(179),
            );
        }
        if let Some((height, x, y)) = arrow {
            ui.painter().add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x, y - height * 0.5),
                    egui::pos2(x + height * 0.6, y),
                    egui::pos2(x, y + height * 0.5),
                ],
                color,
                Stroke::NONE,
            ));
        }
        if response.clicked() {
            action = item.action();
        }
    }
    Painted {
        bounds,
        action,
        inside,
    }
}

pub(crate) struct Outcome<A> {
    pub action: Option<A>,
    pub closed: bool,
}

fn show_popup<A: Copy>(
    parent: &egui::Ui,
    popup: egui::Popup<'_>,
    id: Id,
    reset: bool,
    items: &[Item<'_, A>],
    minimum_width: f32,
    initial_row: Option<usize>,
) -> Outcome<A> {
    let ctx = parent.ctx();
    let mut outcome = Outcome {
        action: None,
        closed: false,
    };
    let session_id = id.with("session");
    let mut session = if reset {
        Session {
            previous_focus: ctx.memory(|memory| memory.focused()),
            ..Session::default()
        }
    } else {
        parent
            .data_mut(|data| data.get_temp::<Session>(session_id))
            .unwrap_or_default()
    };
    if session.levels.is_empty() {
        let slot = initial_row.filter(|&index| index < items.len());
        session.levels.push(Selection {
            slot,
            initial: slot.is_some(),
            pointer: pointer_position(ctx),
            ..Selection::default()
        });
    }
    let mut rectangles = Vec::new();
    let mut shown = false;
    popup
        .width(size(parent, items, minimum_width).x)
        .frame(egui::Frame::NONE)
        .show(|ui| {
            shown = true;
            let focus_id = id.with("input");
            ctx.memory_mut(|memory| memory.set_modal_layer(ui.layer_id()));
            let focus = ui.interact(
                Rect::from_min_size(ui.next_widget_position(), size(ui, items, minimum_width)),
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
                );
            });
            let now = ui.input(|input| input.time);
            for key in keys(ui) {
                let depth = session.parents.len();
                let Some(current) = level_items(items, &session.parents) else {
                    break;
                };
                if matches!(
                    key,
                    egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowRight
                ) || (key == egui::Key::ArrowLeft && depth > 0)
                {
                    for level in &mut session.levels {
                        level.keyboard = true;
                    }
                }
                match key {
                    egui::Key::ArrowUp => session.levels[depth].step(current, true, now),
                    egui::Key::ArrowDown => session.levels[depth].step(current, false, now),
                    egui::Key::ArrowRight => {
                        if let Some(index) = session.levels[depth].slot
                            && let Some(children) = current.get(index).and_then(Item::children)
                        {
                            let mut selection = Selection {
                                pointer: pointer_position(ctx),
                                ..Selection::default()
                            };
                            selection.step(children, false, now);
                            session.parents.push(index);
                            session.levels.push(selection);
                        }
                    }
                    egui::Key::ArrowLeft if depth > 0 => {
                        session.parents.pop();
                        session.levels.pop();
                    }
                    egui::Key::Enter | egui::Key::Space => {
                        outcome.action = session.levels[depth]
                            .slot
                            .and_then(|index| current.get(index))
                            .and_then(Item::action);
                        if outcome.action.is_some() {
                            outcome.closed = true;
                            break;
                        }
                    }
                    egui::Key::Escape => {
                        outcome.closed = true;
                        break;
                    }
                    _ => {}
                }
            }
            let mut painted = paint(
                ui,
                id,
                items,
                &mut session.levels[0],
                !session.parents.is_empty(),
                now,
                minimum_width,
            );
            let mut depth = 0;
            loop {
                rectangles.push(painted.bounds);
                if !outcome.closed
                    && let Some(action) = painted.action
                {
                    outcome.action = Some(action);
                    outcome.closed = true;
                }
                let current = level_items(items, &session.parents[..depth]).unwrap_or(&[]);
                if painted.inside && !session.levels[depth].keyboard {
                    let selected = session.levels[depth].slot;
                    let elapsed = now - session.levels[depth].changed;
                    if elapsed >= 0.1 {
                        let submenu = selected
                            .filter(|&index| current.get(index).and_then(Item::children).is_some());
                        if submenu != session.parents.get(depth).copied() {
                            session.parents.truncate(depth);
                            session.levels.truncate(depth + 1);
                            if let Some(index) = submenu {
                                session.parents.push(index);
                                session.levels.push(Selection {
                                    pointer: pointer_position(ctx),
                                    ..Selection::default()
                                });
                            }
                        }
                    } else {
                        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                            (0.1 - elapsed).max(0.0),
                        ));
                    }
                }
                let Some(&index) = session.parents.get(depth) else {
                    break;
                };
                let Some(children) = current.get(index).and_then(Item::children) else {
                    session.parents.truncate(depth);
                    session.levels.truncate(depth + 1);
                    break;
                };
                let row = Rect::from_min_size(
                    egui::pos2(
                        painted.bounds.left(),
                        painted.bounds.top()
                            + 8.0
                            + current[..index].iter().map(Item::height).sum::<f32>(),
                    ),
                    Vec2::new(painted.bounds.width(), current[index].height()),
                );
                let child_size = size(ui, children, 0.0);
                let viewport = ctx.viewport_rect();
                let x = if painted.bounds.right() + child_size.x <= viewport.right() {
                    painted.bounds.right()
                } else {
                    painted.bounds.left() - child_size.x
                };
                let position = egui::pos2(
                    x.clamp(
                        viewport.left(),
                        (viewport.right() - child_size.x).max(viewport.left()),
                    ),
                    (row.top() - 8.0).clamp(
                        viewport.top(),
                        (viewport.bottom() - child_size.y).max(viewport.top()),
                    ),
                );
                depth += 1;
                if depth > 16 {
                    break;
                }
                painted = egui::Area::new(id.with(("child", depth)))
                    .order(egui::Order::Foreground)
                    .fixed_pos(position)
                    .show(ctx, |ui| {
                        paint(
                            ui,
                            id.with(depth),
                            children,
                            &mut session.levels[depth],
                            session.parents.len() > depth,
                            now,
                            0.0,
                        )
                    })
                    .inner;
            }
        });
    if shown
        && !reset
        && ctx.input(|input| input.pointer.any_click())
        && ctx
            .pointer_interact_pos()
            .is_some_and(|pointer| !rectangles.iter().any(|rect| rect.contains(pointer)))
    {
        outcome.closed = true;
    }
    if shown && !egui::Popup::is_id_open(ctx, id) {
        outcome.closed = true;
    }
    if outcome.closed {
        egui::Popup::close_id(ctx, id);
        restore_focus(ctx, id, &session);
    }
    parent.data_mut(|data| data.insert_temp(session_id, session));
    outcome
}

fn restore_focus(ctx: &egui::Context, id: Id, session: &Session) {
    ctx.memory_mut(|memory| {
        let focus = id.with("input");
        if memory.has_focus(focus) {
            if let Some(previous) = session.previous_focus {
                memory.request_focus(previous);
            } else {
                memory.surrender_focus(focus);
            }
        }
    });
}

pub(crate) fn close(ctx: &egui::Context, id: Id) {
    egui::Popup::close_id(ctx, id);
    if let Some(session) = ctx.data_mut(|data| data.get_temp::<Session>(id.with("session"))) {
        restore_focus(ctx, id, &session);
    }
}

pub(crate) fn show_at<A: Copy>(
    ui: &egui::Ui,
    id: Id,
    anchor: Rect,
    items: &[Item<'_, A>],
) -> Outcome<A> {
    show_at_width(ui, id, anchor, items, 0.0)
}

pub(crate) fn show_at_width<A: Copy>(
    ui: &egui::Ui,
    id: Id,
    anchor: Rect,
    items: &[Item<'_, A>],
    minimum_width: f32,
) -> Outcome<A> {
    let reset = !egui::Popup::is_id_open(ui.ctx(), id);
    let popup = egui::Popup::new(id, ui.ctx().clone(), anchor, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .open_memory(reset.then_some(egui::SetOpenCommand::Bool(true)));
    show_popup(ui, popup, id, reset, items, minimum_width, None)
}

/// Opt-in seed for an ordinary selector; action/avatar menus do not use it.
pub(crate) fn show_selector_at_width<A: Copy>(
    ui: &egui::Ui,
    id: Id,
    anchor: Rect,
    items: &[Item<'_, A>],
    minimum_width: f32,
    initial_row: Option<usize>,
) -> Outcome<A> {
    let reset = !egui::Popup::is_id_open(ui.ctx(), id);
    let popup = egui::Popup::new(id, ui.ctx().clone(), anchor, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .open_memory(reset.then_some(egui::SetOpenCommand::Bool(true)));
    show_popup(ui, popup, id, reset, items, minimum_width, initial_row)
}

pub(crate) struct Row<'a> {
    pub text: &'a str,
    pub enabled: bool,
}

pub(crate) fn show(
    ui: &egui::Ui,
    anchor: &Response,
    id: Id,
    open: Option<egui::SetOpenCommand>,
    rows: &[Row<'_>; 4],
) -> Option<usize> {
    let mut items = Vec::with_capacity(5);
    for (index, row) in rows.iter().enumerate() {
        if index == 2 {
            items.push(Item::Separator);
        }
        items.push(Item::Action {
            text: row.text,
            shortcut: String::new(),
            enabled: row.enabled,
            action: index,
        });
    }
    let reset = !egui::Popup::is_id_open(ui.ctx(), id) || open.is_some();
    show_popup(
        ui,
        egui::Popup::menu(anchor)
            .id(id)
            .open_memory(open)
            .at_pointer_fixed(),
        id,
        reset,
        &items,
        0.0,
        None,
    )
    .action
}
