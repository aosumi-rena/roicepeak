//! Shortcut dispatch and independent binding editor.
use crate::app::{
    editor::EditorApp,
    shortcuts::{Chord, Command},
};
use eframe::egui;

impl EditorApp {
    pub(crate) fn dispatch_shortcuts(&mut self, ctx: &egui::Context) {
        if !ctx.input(|input| input.focused) {
            self.ime_composing = false;
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for event in &events {
            match event {
                egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                    self.ime_composing = !text.is_empty()
                }
                egui::Event::Ime(egui::ImeEvent::Commit(_)) => self.ime_composing = false,
                #[expect(deprecated)]
                egui::Event::Ime(egui::ImeEvent::Disabled) => self.ime_composing = false,
                _ => {}
            }
        }
        if self.ime_composing
            || events.iter().any(|e| matches!(e, egui::Event::Ime(_)))
            || egui::Popup::is_any_open(ctx)
            || crate::app::timestamp_popup::active(ctx)
        {
            return;
        }
        for event in events {
            let egui::Event::Key {
                key,
                modifiers,
                pressed: true,
                repeat,
                ..
            } = event
            else {
                continue;
            };
            let Some(chord) = Chord::from_egui(key, modifiers) else {
                continue;
            };
            if self.shortcut_settings
                && self
                    .shortcut_dialog
                    .capture_key(ctx, &self.shortcuts, chord)
            {
                consume(ctx, chord);
                return;
            }
            if self.pending.is_some()
                || self.preferences
                || self.dictionary
                || self.about
                || self.error.is_some()
                || crate::app::emotion_presets::active(ctx)
                || self.export.active()
                || self.resource_settings
                || self.experimental_settings
                || self.voice_replacement.is_some()
            {
                return;
            }
            if key == egui::Key::Escape
                && modifiers == egui::Modifiers::NONE
                && self.popup.is_some()
            {
                self.popup = None;
                consume(ctx, chord);
                return;
            }
            if self.shortcut_settings {
                return;
            }
            let Some(command) = self.shortcuts.resolve(chord) else {
                continue;
            };
            let script_focused = self.focused_script_selection(ctx).is_some();
            if repeat && !(script_focused && matches!(command, Command::Undo | Command::Redo)) {
                continue;
            }
            if matches!(
                command,
                Command::Copy | Command::Cut | Command::Paste | Command::SelectAll
            ) && !script_focused
            {
                continue;
            }
            if script_focused
                && matches!(
                    command,
                    Command::Copy
                        | Command::Cut
                        | Command::Paste
                        | Command::SelectAll
                        | Command::Undo
                        | Command::Redo
                )
            {
                consume(ctx, chord);
                self.command(command, ctx);
                return;
            }
            // Other fields retain their local editing commands.
            if ctx.egui_wants_keyboard_input()
                && !ctx.memory(|memory| memory.has_focus(crate::app::script_view::focus_id()))
                && (matches!(
                    command,
                    Command::Undo
                        | Command::Redo
                        | Command::PreviousBlock
                        | Command::NextBlock
                        | Command::Copy
                        | Command::Cut
                        | Command::Paste
                        | Command::SelectAll
                        | Command::DeleteSelection
                ) || (!chord.modifiers.control
                    && !chord.modifiers.alt
                    && !chord.modifiers.super_key
                    && matches!(
                        chord.key,
                        crate::app::shortcuts::Key::Letter(_)
                            | crate::app::shortcuts::Key::Digit(_)
                            | crate::app::shortcuts::Key::Symbol(_)
                            | crate::app::shortcuts::Key::Space
                            | crate::app::shortcuts::Key::Enter
                            | crate::app::shortcuts::Key::Tab
                            | crate::app::shortcuts::Key::Backspace
                            | crate::app::shortcuts::Key::Delete
                            | crate::app::shortcuts::Key::Up
                            | crate::app::shortcuts::Key::Down
                            | crate::app::shortcuts::Key::Left
                            | crate::app::shortcuts::Key::Right
                            | crate::app::shortcuts::Key::Home
                            | crate::app::shortcuts::Key::End
                            | crate::app::shortcuts::Key::PageUp
                            | crate::app::shortcuts::Key::PageDown
                    )))
            {
                continue;
            }
            consume(ctx, chord);
            self.command(command, ctx);
            return;
        }
    }

    pub(crate) fn shortcut_window(&mut self, ctx: &egui::Context) {
        let mut state = std::mem::take(&mut self.shortcut_dialog);
        state.show(self, ctx);
        self.shortcut_dialog = state;
    }
}

fn consume(ctx: &egui::Context, chord: Chord) {
    ctx.input_mut(|input| input.events.retain(|event| !matches!(event,
        egui::Event::Key { key, modifiers, pressed: true, .. } if Chord::from_egui(*key, *modifiers) == Some(chord))));
}
