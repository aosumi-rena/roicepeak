//! Boundary key callbacks run before the local text editor consumes the key.
use crate::app::{editor::EditorApp, transport::Transport};
use eframe::egui;

impl EditorApp {
    pub(crate) fn script_boundary_keys(&mut self, ui: &egui::Ui) {
        let ctx = ui.ctx();
        if !ui.is_enabled()
            || self.ime_composing
            || self.popup.is_some()
            || egui::Popup::is_any_open(ctx)
            || ctx.input(|input| {
                input
                    .events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Ime(_)))
            })
        {
            return;
        }
        let Some((index, _, range)) = self.focused_script_selection(ctx) else {
            return;
        };
        let [first, last] = range.sorted_cursors();
        if first.index != last.index {
            return;
        }
        let caret = usize::from(first.index);
        let key = ctx.input(|input| {
            input.events.iter().enumerate().find_map(|(at, event)| {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                else {
                    return None;
                };
                // Native allows either Ctrl or Alt separately, but excludes both.
                if modifiers.ctrl && modifiers.alt || modifiers.mac_cmd {
                    return None;
                }
                matches!(key, egui::Key::Backspace | egui::Key::Delete).then_some((at, *key))
            })
        });
        let Some((event_at, key)) = key else {
            return;
        };
        let length = self.project.blocks[index].text.chars().count();
        let left = match key {
            egui::Key::Backspace if caret == 0 && index != 0 => index - 1,
            egui::Key::Delete if caret == length && index + 1 < self.project.blocks.len() => index,
            _ => return,
        };
        let destination = self.project.blocks[left].text.chars().count();
        // Validate every authored/state limit before starting a transaction.
        let merged = self.project.blocks[left].merged_with(&self.project.blocks[left + 1]);
        ctx.input_mut(|input| {
            input.events.remove(event_at);
        });
        match merged {
            Err(error) => self.error = Some(error),
            Ok(merged) => {
                let id = merged.id;
                self.begin_edit();
                self.project.blocks[left] = merged;
                self.project.blocks.remove(left + 1);
                // Native removal drops the removed member and clears current
                // when it referred to that block. It does not pick a successor.
                self.selection.prune(&self.project);
                if self.transport != Transport::Playing {
                    self.selection.focus_current(id);
                    self.script_caret(ctx, left, destination);
                }
            }
        }
    }
}
