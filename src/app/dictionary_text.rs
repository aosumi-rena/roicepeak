use eframe::egui::{self, Event, ImeEvent, Key, Modifiers};

#[derive(Default)]
pub(super) struct Composition {
    active: bool,
    blocked: bool,
    blurred: bool,
}

impl Composition {
    pub(super) fn keys(&mut self, ui: &mut egui::Ui, focused: bool) -> (bool, bool) {
        if !ui.is_enabled() {
            *self = Self::default();
            return (false, false);
        }
        if !focused {
            self.blocked = self.active;
            self.active = false;
            return (false, false);
        }
        ui.input_mut(|input| {
            let previous = self.active;
            let mut ime_event = false;
            for event in &input.events {
                if let Event::Ime(event) = event {
                    ime_event = true;
                    match event {
                        ImeEvent::Preedit { text, .. } | ImeEvent::Commit(text)
                            if text == "\n" || text == "\r" => {}
                        ImeEvent::Preedit { text, .. } => self.active = !text.is_empty(),
                        ImeEvent::Commit(_) => self.active = false,
                        #[expect(deprecated)]
                        ImeEvent::Disabled => self.active = false,
                        _ => {}
                    }
                }
            }
            self.blocked = previous || self.active || ime_event;
            if self.blocked {
                return (false, false);
            }
            (
                input.consume_key(Modifiers::NONE, Key::Escape),
                input.consume_key(Modifiers::NONE, Key::Enter),
            )
        })
    }

    pub(super) fn commit(&mut self, lost_focus: bool, enter: bool) -> bool {
        self.blurred |= lost_focus;
        if !self.busy() && (self.blurred || enter) {
            self.blurred = false;
            true
        } else {
            false
        }
    }

    pub(super) fn busy(&self) -> bool {
        self.active || self.blocked
    }

    pub(super) fn cancel(&mut self) {
        *self = Self::default();
    }
}
