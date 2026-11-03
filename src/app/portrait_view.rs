//! Portrait gestures, retained transforms and native context actions.
#[path = "portrait_view_input.rs"]
mod input;
#[path = "portrait_view_math.rs"]
mod math;
#[path = "portrait_view_store.rs"]
mod store;

use crate::app::{
    catalog::Portrait,
    i18n::{self, Locale},
    native_menu::{self, Item},
};
use eframe::egui::{self, Pos2, Rect, Sense, Ui, Vec2};
use math::View;

pub(super) fn raw_input(ctx: &egui::Context, input: &mut egui::RawInput) {
    input::raw_input(ctx, input);
}

struct Drag {
    identity: String,
    last: [i32; 2],
    button: egui::PointerButton,
}

struct Menu {
    identity: Option<String>,
    anchor: Rect,
}

#[derive(Default)]
pub(super) struct State {
    store: store::Store,
    drag: Option<Drag>,
    menu: Option<Menu>,
    current: Option<String>,
}

#[derive(Clone, Copy)]
enum Action {
    Reset,
    Close,
}

fn local(point: Pos2, panel: Rect) -> [i32; 2] {
    [
        (point.x - panel.left()) as i32,
        (point.y - panel.top()) as i32,
    ]
}

impl State {
    pub fn take_problem(&mut self) -> Option<String> {
        self.store.problem.take()
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        panel: Rect,
        identity: Option<&str>,
        portrait: Option<&Portrait>,
        locale: Locale,
    ) -> (Option<Rect>, bool) {
        let card = super::frame(panel);
        let menu_id = ui.id().with("portrait-context");
        let identity_changed = self.current.as_deref() != identity;
        if identity_changed {
            self.current = identity.map(str::to_owned);
            self.drag = None;
            self.menu = None;
            native_menu::close(ui.ctx(), menu_id);
        }
        if !ui.is_enabled() {
            self.drag = None;
            self.menu = None;
            native_menu::close(ui.ctx(), menu_id);
        }
        let close_rect = Rect::from_min_size(
            panel.min + Vec2::new(panel.width() - 64.0, 32.0),
            Vec2::splat(32.0),
        );
        let hit = card.intersect(ui.clip_rect());
        let response = ui.interact(card, ui.id().with("portrait-view"), Sense::click_and_drag());
        let mut view = identity.zip(portrait).map(|(identity, portrait)| {
            self.store.load_once();
            let saved = self.store.views.get(identity).copied();
            if saved.is_some_and(|view| !view.valid_for(portrait)) {
                self.store
                    .block_writes("Stored portrait transform exceeds the artwork bounds".into());
            }
            let mut view = saved
                .filter(|view| view.valid_for(portrait))
                .unwrap_or_else(|| View::initial(card, portrait));
            if identity_changed {
                view.restore_size(card, portrait);
            } else {
                view.resize(card, portrait);
            }
            view
        });
        let mut persist = false;
        if let (Some(identity), Some(portrait), Some(view)) = (identity, portrait, view.as_mut()) {
            input::register(ui, card, identity);
            let original = self.store.views.get(identity);
            persist |= !identity_changed && original.is_some_and(|original| original != view);
            if ui.is_enabled() {
                let events = ui.input(|state| {
                    state
                        .events
                        .iter()
                        .filter(|event| {
                            matches!(
                                event,
                                egui::Event::PointerMoved(_)
                                    | egui::Event::PointerButton { .. }
                                    | egui::Event::PointerGone
                            )
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                });
                for event in &events {
                    match event {
                        egui::Event::PointerButton {
                            pos,
                            button,
                            pressed: true,
                            ..
                        } if hit.contains(*pos)
                            && !close_rect.contains(*pos)
                            && ui.ctx().layer_id_at(*pos) == Some(ui.layer_id()) =>
                        {
                            self.drag = Some(Drag {
                                identity: identity.to_owned(),
                                last: local(*pos, panel),
                                button: *button,
                            });
                        }
                        egui::Event::PointerMoved(point) => {
                            if let Some(drag) = &mut self.drag {
                                if drag.identity == identity {
                                    let point = local(*point, panel);
                                    view.translate(
                                        [
                                            point[0].wrapping_sub(drag.last[0]),
                                            point[1].wrapping_sub(drag.last[1]),
                                        ],
                                        card,
                                        portrait,
                                    );
                                    drag.last = point;
                                }
                            }
                        }
                        egui::Event::PointerButton {
                            button,
                            pressed: false,
                            ..
                        } => {
                            if self
                                .drag
                                .as_ref()
                                .is_some_and(|drag| drag.button == *button)
                            {
                                self.drag = None;
                                persist = true;
                            }
                        }
                        egui::Event::PointerGone => self.drag = None,
                        _ => {}
                    }
                }
                if self
                    .drag
                    .as_ref()
                    .is_some_and(|drag| !ui.input(|state| state.pointer.button_down(drag.button)))
                {
                    self.drag = None;
                }
                for step in input::take(ui, card, identity) {
                    view.zoom(step.delta_y, local(step.pointer, panel), card, portrait);
                    persist = true;
                }
                if response.double_clicked()
                    || response.double_clicked_by(egui::PointerButton::Secondary)
                    || response.double_clicked_by(egui::PointerButton::Middle)
                {
                    *view = View::initial(card, portrait);
                    self.drag = None;
                    persist = true;
                }
            } else {
                let _ = input::take(ui, card, identity);
            }
        } else {
            input::unregister(ui.ctx());
            self.drag = None;
        }
        if ui.is_enabled() && response.secondary_clicked() {
            if let Some(point) = response.interact_pointer_pos() {
                if hit.contains(point) && !close_rect.contains(point) {
                    self.menu = Some(Menu {
                        identity: identity.map(str::to_owned),
                        anchor: Rect::from_min_size(point, Vec2::ZERO),
                    });
                }
            }
        }
        let mut close = false;
        if let Some(menu) = &self.menu {
            let items = [
                Item::Action {
                    text: i18n::tr(locale, "表示をデフォルトに戻す", "Reset view"),
                    shortcut: String::new(),
                    enabled: true,
                    action: Action::Reset,
                },
                Item::Action {
                    text: i18n::tr(locale, "閉じる", "Close"),
                    shortcut: String::new(),
                    enabled: true,
                    action: Action::Close,
                },
            ];
            let outcome = native_menu::show_at(ui, menu_id, menu.anchor, &items);
            let matches_identity = menu.identity.as_deref() == identity;
            match outcome.action.filter(|_| matches_identity) {
                Some(Action::Reset) => {
                    if let (Some(portrait), Some(view)) = (portrait, view.as_mut()) {
                        *view = View::initial(card, portrait);
                        self.drag = None;
                        persist = true;
                    }
                }
                Some(Action::Close) => close = true,
                None => {}
            }
            if outcome.closed {
                self.menu = None;
            }
        }
        let rect = view
            .zip(portrait)
            .map(|(view, portrait)| view.image_rect(panel, portrait));
        if let (Some(identity), Some(view)) = (identity, view) {
            self.store.record(identity, view, persist);
        }
        (rect, close)
    }
}
