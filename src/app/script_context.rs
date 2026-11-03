//! Script editor actions use the clicked editor's captured original range.
use crate::app::{
    editor::EditorApp,
    i18n,
    native_menu::{self, Item},
    sentence_edit::Fragment,
    shortcuts::Command,
    transport::Transport,
};
use eframe::egui::{self, Rect};
use std::sync::Arc;

#[path = "script_registration.rs"]
mod registration;

#[derive(Clone)]
pub(crate) struct Capture {
    block: u64,
    source: String,
    range: egui::text::CCursorRange,
    anchor: Rect,
    register_surface: String,
}

fn id() -> egui::Id {
    egui::Id::new("script-context-menu")
}
fn capture_id() -> egui::Id {
    id().with("capture")
}

/// Capture before TextEdit: its pointer selector treats secondary presses as
/// ordinary presses and would otherwise collapse the existing selected range.
pub(crate) fn capture(
    ui: &egui::Ui,
    block: &crate::app::document::Block,
    rect: Rect,
) -> Option<Capture> {
    if !ui.is_enabled() || egui::Popup::is_any_open(ui.ctx()) {
        return None;
    }
    let position = ui.input(|input| {
        input
            .pointer
            .secondary_pressed()
            .then(|| input.pointer.interact_pos())
            .flatten()
    })?;
    if !rect.intersect(ui.clip_rect()).contains(position) {
        return None;
    }
    let editor = egui::Id::new(("block-text", block.id));
    let mut range = egui::TextEdit::load_state(ui.ctx(), editor)
        .and_then(|state| state.cursor.char_range())
        .unwrap_or_else(|| egui::text::CCursorRange::one(egui::text::CCursor::new(0)));
    // New/Open can reuse a block ID before TextEdit has clamped its old state.
    // Change indices only, retaining orientation and each cursor's row affinity.
    let limit = egui::text::CCursor::new(block.text.chars().count()).index;
    range.primary.index = range.primary.index.min(limit);
    range.secondary.index = range.secondary.index.min(limit);
    let [first, last] = range.sorted_cursors();
    let selection = usize::from(first.index)..usize::from(last.index);
    let text_at = |scalar| {
        block
            .text
            .char_indices()
            .nth(scalar)
            .map_or(block.text.len(), |(byte, _)| byte)
    };
    let register_surface = block
        .text
        .get(text_at(selection.start)..text_at(selection.end))
        .unwrap_or("")
        .to_owned();
    Some(Capture {
        block: block.id,
        source: block.text.clone(),
        range,
        anchor: Rect::from_min_size(position, egui::Vec2::ZERO),
        register_surface,
    })
}

#[derive(Clone, Copy)]
enum Action {
    Cut,
    Copy,
    Paste,
    PasteText,
    Delete,
    SelectAll,
    Play,
    Loop,
    Register,
    Export,
}

impl EditorApp {
    pub(crate) fn open_script_context(&mut self, ctx: &egui::Context, capture: Capture) {
        // TextEdit may already have processed this secondary press. Put the
        // captured range back without triggering an authored edit or caret seek.
        let editor = egui::Id::new(("block-text", capture.block));
        let mut state = egui::TextEdit::load_state(ctx, editor).unwrap_or_default();
        state.cursor.set_char_range(Some(capture.range));
        state.store(ctx, editor);
        native_menu::close(ctx, id());
        ctx.data_mut(|data| data.insert_temp(capture_id(), Arc::new(capture)));
    }

    pub(crate) fn script_context(&mut self, ui: &egui::Ui) {
        if !ui.is_enabled() {
            native_menu::close(ui.ctx(), id());
            ui.data_mut(|data| data.remove::<Arc<Capture>>(capture_id()));
            return;
        }
        let Some(capture) = ui.data_mut(|data| data.get_temp::<Arc<Capture>>(capture_id())) else {
            return;
        };
        if !self
            .project
            .blocks
            .iter()
            .any(|block| block.id == capture.block && block.text == capture.source)
        {
            native_menu::close(ui.ctx(), id());
            ui.data_mut(|data| data.remove::<Arc<Capture>>(capture_id()));
            return;
        }
        let [first, last] = capture.range.sorted_cursors();
        let selected = first.index != last.index;
        let register = i18n::format(
            self.locale,
            "Register \"{}\"...",
            &[&capture.register_surface],
        );
        let row = |text: &'static str, enabled, action| Item::Action {
            text,
            shortcut: String::new(),
            enabled,
            action,
        };
        let play = if self.transport == Transport::Playing {
            Command::PlayPause
        } else {
            Command::PlayBlock
        };
        let command = |text: &'static str, kind, action| Item::Action {
            text,
            shortcut: self
                .shortcuts
                .primary_chord(kind)
                .map(|chord| chord.label())
                .unwrap_or_default(),
            enabled: true,
            action,
        };
        let items = vec![
            row(i18n::text(self.locale, "Cut"), true, Action::Cut),
            row(i18n::text(self.locale, "Copy"), selected, Action::Copy),
            row(i18n::text(self.locale, "Paste"), true, Action::Paste),
            row(
                i18n::text(self.locale, "Paste Text"),
                true,
                Action::PasteText,
            ),
            row(i18n::text(self.locale, "Delete"), true, Action::Delete),
            Item::Separator,
            row(
                i18n::text(self.locale, "Select All"),
                true,
                Action::SelectAll,
            ),
            Item::Separator,
            command(i18n::text(self.locale, play.label()), play, Action::Play),
            command(
                i18n::text(self.locale, Command::ToggleLoop.label()),
                Command::ToggleLoop,
                Action::Loop,
            ),
            Item::Separator,
            Item::Action {
                text: &register,
                shortcut: if selected {
                    self.shortcuts
                        .primary_chord(Command::RegisterWord)
                        .map(|chord| chord.label())
                        .unwrap_or_default()
                } else {
                    String::new()
                },
                enabled: selected,
                action: Action::Register,
            },
            Item::Separator,
            command(
                i18n::text(self.locale, Command::ExportBlock.label()),
                Command::ExportBlock,
                Action::Export,
            ),
        ];
        let outcome = native_menu::show_at(ui, id(), capture.anchor, &items);
        if outcome.closed {
            ui.data_mut(|data| data.remove::<Arc<Capture>>(capture_id()));
        }
        if let Some(action) = outcome.action {
            if let Err(error) = self.script_context_action(ui.ctx(), &capture, action) {
                self.error = Some(error);
            }
        }
    }

    fn script_context_action(
        &mut self,
        ctx: &egui::Context,
        capture: &Capture,
        action: Action,
    ) -> Result<(), String> {
        let index = self
            .project
            .blocks
            .iter()
            .position(|block| block.id == capture.block && block.text == capture.source)
            .ok_or("Context text changed before its action")?;
        let editor = egui::Id::new(("block-text", capture.block));
        let mut state = egui::TextEdit::load_state(ctx, editor).unwrap_or_default();
        state.cursor.set_char_range(Some(capture.range));
        state.store(ctx, editor);
        ctx.memory_mut(|memory| memory.request_focus(editor));
        let [first, last] = capture.range.sorted_cursors();
        let selection = usize::from(first.index)..usize::from(last.index);
        match action {
            Action::Cut | Action::Copy => self.copy_script_text(ctx, matches!(action, Action::Cut)),
            Action::Paste | Action::PasteText => {
                let payload = if matches!(action, Action::Paste) {
                    crate::app::text_clipboard::read()?
                } else {
                    None
                };
                let fragments = if let Some(payload) = payload {
                    payload.sentences
                } else {
                    let Some(text) = read_plain_clipboard()?.filter(|text| !text.is_empty()) else {
                        return Ok(());
                    };
                    vec![Fragment {
                        text,
                        completes_sentence: false,
                        words: Vec::new(),
                        ssml: None,
                    }]
                };
                let caret = selection.start
                    + fragments
                        .iter()
                        .map(|fragment| fragment.text.chars().count())
                        .sum::<usize>();
                self.replace_script_text(ctx, index, selection, &fragments)?;
                self.script_caret(ctx, index, caret);
                Ok(())
            }
            Action::Delete => {
                if selection.is_empty() {
                    Ok(())
                } else {
                    self.replace_script_text(ctx, index, selection, &[])
                }
            }
            Action::SelectAll => {
                let mut state = egui::TextEdit::load_state(ctx, editor).unwrap_or_default();
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(capture.source.chars().count()),
                    )));
                state.store(ctx, editor);
                Ok(())
            }
            Action::Register => {
                // Preparation may have refreshed canonical words while this
                // menu was open. Build the draft from the accepted current tree.
                if let Some(entry) = registration::entry(&self.project.blocks[index], selection)? {
                    self.begin_dictionary_window();
                    self.dictionary = true;
                    self.dictionary_ui.begin_draft(entry, None);
                }
                Ok(())
            }
            Action::Play => {
                if self.transport != Transport::Playing {
                    self.selection.focus_current(capture.block);
                }
                self.command(
                    if self.transport == Transport::Playing {
                        Command::PlayPause
                    } else {
                        Command::PlayBlock
                    },
                    ctx,
                );
                Ok(())
            }
            Action::Loop => {
                self.command(Command::ToggleLoop, ctx);
                Ok(())
            }
            Action::Export => {
                self.export.open(
                    crate::app::export::Scope::Blocks(vec![capture.block]),
                    &self.project.export_settings,
                );
                Ok(())
            }
        }
    }
}

#[cfg(windows)]
fn read_plain_clipboard() -> Result<Option<String>, String> {
    use clipboard_win::{Clipboard, formats::CF_UNICODETEXT, raw};
    let _clipboard = Clipboard::new().map_err(|error| format!("Cannot open clipboard: {error}"))?;
    if !raw::is_format_avail(CF_UNICODETEXT) {
        return Ok(None);
    }
    let size = raw::size(CF_UNICODETEXT)
        .ok_or("Cannot measure clipboard text")?
        .get();
    if size > 2 * (crate::app::sentence_edit::MAX_TEXT_BYTES + 1) + 4096 {
        return Err("Clipboard text allocation exceeds its limit".into());
    }
    let mut bytes = Vec::new();
    raw::get_string(&mut bytes).map_err(|error| format!("Cannot read clipboard text: {error}"))?;
    if bytes.len() > crate::app::sentence_edit::MAX_TEXT_BYTES {
        return Err("Clipboard text exceeds its limit".into());
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(not(windows))]
fn read_plain_clipboard() -> Result<Option<String>, String> {
    Err("Script context clipboard reading is currently implemented for Windows".into())
}
