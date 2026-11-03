use crate::app::{
    editor::EditorApp,
    sentence_edit::{self, Fragment},
    shortcuts::Command,
    text_clipboard::{self, Payload},
};
use eframe::egui;
use std::ops::Range;

impl EditorApp {
    pub(crate) fn focused_script_selection(
        &self,
        ctx: &egui::Context,
    ) -> Option<(usize, egui::Id, egui::text::CCursorRange)> {
        self.project
            .blocks
            .iter()
            .enumerate()
            .find_map(|(index, block)| {
                let id = egui::Id::new(("block-text", block.id));
                if !ctx.memory(|memory| memory.has_focus(id)) {
                    return None;
                }
                let range = egui::TextEdit::load_state(ctx, id)?.cursor.char_range()?;
                Some((index, id, range))
            })
    }

    pub(crate) fn script_clipboard_events(&mut self, ctx: &egui::Context) {
        if self.focused_script_selection(ctx).is_none() {
            return;
        }
        if self.pending.is_some()
            || self.error.is_some()
            || self.preferences
            || self.dictionary
            || self.about
            || self.shortcut_settings
            || self.resource_settings
            || self.experimental_settings
            || self.voice_replacement.is_some()
            || self.export.active()
            || crate::app::emotion_presets::active(ctx)
            || egui::Popup::is_any_open(ctx)
        {
            ctx.input_mut(|input| {
                input.events.retain(|event| {
                    !matches!(
                        event,
                        egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_)
                    )
                })
            });
            return;
        }
        if self.ime_composing
            || ctx.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Ime(_)))
            })
        {
            return;
        }
        let events = ctx.input_mut(|input| {
            let mut found = Vec::new();
            input.events.retain(|event| {
                if matches!(
                    event,
                    egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_)
                ) {
                    found.push(event.clone());
                    false
                } else {
                    true
                }
            });
            found
        });
        for event in events {
            let result = match event {
                egui::Event::Copy => self.copy_script_text(ctx, false),
                egui::Event::Cut => self.copy_script_text(ctx, true),
                egui::Event::Paste(text) => self.paste_script_text(ctx, text),
                _ => unreachable!(),
            };
            if let Err(error) = result {
                self.error = Some(error);
                break;
            }
        }
    }

    pub(crate) fn copy_script_text(
        &mut self,
        ctx: &egui::Context,
        cut: bool,
    ) -> Result<(), String> {
        let (index, _, range) = self
            .focused_script_selection(ctx)
            .ok_or("Focus the block text editor first")?;
        let [first, last] = range.sorted_cursors();
        let selection = usize::from(first.index)..usize::from(last.index);
        if selection.is_empty() {
            return Ok(());
        }
        let block = &self.project.blocks[index];
        let Some(payload) = Payload::from_block(block, selection.clone())? else {
            return Ok(());
        };
        let replacement = cut
            .then(|| sentence_edit::replace(block, selection.clone(), &[]))
            .transpose()?;
        text_clipboard::write(ctx, &payload)?;
        if let Some(replacement) = replacement {
            self.begin_edit();
            self.project.blocks[index] = replacement;
            self.script_caret(ctx, index, selection.start);
        }
        Ok(())
    }

    pub(crate) fn paste_script_text(
        &mut self,
        ctx: &egui::Context,
        fallback: String,
    ) -> Result<(), String> {
        let (index, _, range) = self
            .focused_script_selection(ctx)
            .ok_or("Focus the block text editor first")?;
        let fragments = if let Some(payload) = text_clipboard::read()? {
            payload.sentences
        } else if fallback.is_empty() {
            return Ok(());
        } else {
            vec![Fragment {
                text: fallback,
                completes_sentence: false,
                words: Vec::new(),
                ssml: None,
            }]
        };
        let [first, last] = range.sorted_cursors();
        let selection = usize::from(first.index)..usize::from(last.index);
        let added: usize = fragments
            .iter()
            .map(|fragment| fragment.text.chars().count())
            .sum();
        self.replace_script_text(ctx, index, selection.clone(), &fragments)?;
        self.script_caret(ctx, index, selection.start + added);
        Ok(())
    }

    pub(crate) fn replace_script_text(
        &mut self,
        ctx: &egui::Context,
        index: usize,
        selection: Range<usize>,
        fragments: &[Fragment],
    ) -> Result<(), String> {
        let block = self
            .project
            .blocks
            .get(index)
            .ok_or("Block is no longer present")?;
        let replacement = sentence_edit::replace(block, selection.clone(), fragments)?;
        self.begin_edit();
        self.project.blocks[index] = replacement;
        self.script_caret(ctx, index, selection.start);
        Ok(())
    }

    pub(crate) fn script_caret(&self, ctx: &egui::Context, index: usize, caret: usize) {
        let id = egui::Id::new(("block-text", self.project.blocks[index].id));
        let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
        state.clear_undoer();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(caret),
            )));
        state.store(ctx, id);
        ctx.memory_mut(|memory| memory.request_focus(id));
    }

    pub(crate) fn script_text_command(
        &mut self,
        command: Command,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        let (_, id, _) = self
            .focused_script_selection(ctx)
            .ok_or("Focus the block text editor first")?;
        match command {
            Command::Copy | Command::Cut => {
                ctx.input_mut(|input| {
                    input
                        .events
                        .retain(|event| !matches!(event, egui::Event::Copy | egui::Event::Cut))
                });
                self.copy_script_text(ctx, command == Command::Cut)
            }
            Command::Paste => {
                let event = ctx.input_mut(|input| {
                    let at = input
                        .events
                        .iter()
                        .position(|event| matches!(event, egui::Event::Paste(_)))?;
                    Some(input.events.remove(at))
                });
                if let Some(egui::Event::Paste(text)) = event {
                    self.paste_script_text(ctx, text)
                } else {
                    ctx.memory_mut(|memory| memory.request_focus(id));
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                    Ok(())
                }
            }
            _ => Err("Not a clipboard command".into()),
        }
    }
}
