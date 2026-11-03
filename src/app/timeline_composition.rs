use eframe::egui::{self, Context, Event, Id, ImeEvent, Key, Modifiers};

#[derive(Clone, Default)]
pub(super) struct Composition {
    active: bool,
    blocked: bool,
    finish_pending: bool,
    was_focused: bool,
    frame: Option<u64>,
}

impl Composition {
    pub(super) fn refresh(&mut self, ctx: &Context, field: Option<Id>, enabled: bool) {
        let focused = field.is_some_and(|id| ctx.memory(|m| m.has_focus(id)));
        self.observe(ctx, focused, enabled);
    }

    fn observe(&mut self, ctx: &Context, focused: bool, enabled: bool) {
        if !enabled {
            *self = Self::default();
            return;
        }
        let frame = ctx.cumulative_frame_nr();
        if self.frame == Some(frame) {
            return;
        }
        self.frame = Some(frame);
        let focused = focused && ctx.input(|i| i.focused);
        let previous = self.active;
        self.finish_pending |= self.was_focused && !focused;
        let observe_events = focused || previous || self.was_focused;
        if !focused {
            self.active = false;
        }
        let mut ime_event = false;
        if observe_events {
            ctx.input(|input| {
                for event in &input.events {
                    if let Event::Ime(event) = event {
                        ime_event = true;
                        match event {
                            ImeEvent::Preedit { text, .. } | ImeEvent::Commit(text)
                                if text == "\n" || text == "\r" => {}
                            ImeEvent::Preedit { text, .. } if focused => {
                                self.active = !text.is_empty();
                            }
                            ImeEvent::Commit(_) => self.active = false,
                            #[expect(deprecated)]
                            ImeEvent::Disabled => self.active = false,
                            _ => {}
                        }
                    }
                }
            });
        }
        self.blocked = previous || self.active || ime_event;
        self.was_focused = focused;
        if self.blocked && !self.active {
            ctx.request_repaint();
        }
    }

    pub(super) fn keys(&mut self, ui: &mut egui::Ui, focused: bool) -> (bool, bool) {
        self.observe(ui.ctx(), focused, ui.is_enabled());
        if !ui.is_enabled() || !focused || self.active || self.blocked {
            return (false, false);
        }
        ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        })
    }

    pub(super) fn finish(&mut self, requested: bool) -> bool {
        self.finish_pending |= requested;
        self.finish_pending && !self.active && !self.blocked
    }
}
