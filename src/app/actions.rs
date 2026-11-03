//! Shared editor commands for toolbar, menus and key bindings.
use crate::app::{
    editor::{EditorApp, PendingAction, Popup},
    shortcuts::Command,
};
use eframe::egui;

impl EditorApp {
    pub(crate) fn command(&mut self, command: Command, ctx: &egui::Context) {
        use Command::*;
        if !self.dispatching_audio
            && matches!(command, PlayPause | PlayBlock | Export | ExportBlock)
        {
            if self.audio_commands.len() < 16 {
                self.audio_commands.push(command);
            }
            ctx.request_repaint();
            return;
        }
        let result = match command {
            NewProject => {
                self.request_action(PendingAction::New, ctx);
                Ok(())
            }
            OpenProject => {
                self.request_action(PendingAction::Open, ctx);
                Ok(())
            }
            SaveProject => {
                self.save(false);
                Ok(())
            }
            SaveProjectAs => {
                self.save(true);
                Ok(())
            }
            ImportText => {
                self.import();
                Ok(())
            }
            ImportSrt => {
                self.request_action(PendingAction::ImportSrt, ctx);
                Ok(())
            }
            ImportCsv => self.import_format(crate::app::imports::Format::Csv),
            ImportSsml => {
                self.request_action(PendingAction::ImportSsml, ctx);
                Ok(())
            }
            Export => {
                self.export.open(
                    crate::app::export::Scope::Project,
                    &self.project.export_settings,
                );
                Ok(())
            }
            ExportBlock => {
                match self
                    .selected()
                    .and_then(|index| self.project.blocks.get(index))
                {
                    Some(block) => {
                        self.export.open(
                            crate::app::export::Scope::Blocks(vec![block.id]),
                            &self.project.export_settings,
                        );
                        Ok(())
                    }
                    None => Err("Select a block first".into()),
                }
            }
            Undo => {
                self.undo(ctx);
                Ok(())
            }
            Redo => {
                self.redo(ctx);
                Ok(())
            }
            Preferences => {
                self.preferences = true;
                Ok(())
            }
            About => {
                self.about = true;
                Ok(())
            }
            Dictionary => {
                self.dictionary = true;
                Ok(())
            }
            RegisterWord => {
                self.dictionary = true;
                self.dictionary_ui
                    .begin_draft(crate::app::dictionary::Entry::default(), None);
                Ok(())
            }
            KeyboardShortcuts => {
                self.shortcut_settings = true;
                Ok(())
            }
            FindReplace => {
                self.find_replace.reveal();
                Ok(())
            }
            ShowSpeed => {
                self.toggle(Popup::Speed);
                Ok(())
            }
            ShowPitch => {
                self.toggle(Popup::Pitch);
                Ok(())
            }
            ShowPause => {
                self.toggle(Popup::Pause);
                Ok(())
            }
            ShowEmotion => {
                self.toggle(Popup::Emotion);
                Ok(())
            }
            PlayPause => self.play_pause(),
            PlayBlock => self.play_current_block(),
            ToggleLoop => {
                self.loop_playback = !self.loop_playback;
                self.player.repeat(self.loop_playback);
                Ok(())
            }
            SkipStart => {
                self.playhead = 0.0;
                self.player.full_range();
                self.player.seek(0.0);
                Ok(())
            }
            SkipEnd => {
                self.playhead = self.playback_duration();
                self.player.pause();
                self.transport = crate::app::transport::Transport::Paused;
                self.player.full_range();
                self.player.seek(self.playhead);
                Ok(())
            }
            PreviousBlock => {
                self.move_block(false);
                Ok(())
            }
            NextBlock => {
                self.move_block(true);
                Ok(())
            }
            Copy | Cut | Paste | SelectAll => self.text_command(command, ctx),
            DeleteSelection => {
                if ctx.egui_wants_keyboard_input()
                    && !ctx.memory(|memory| memory.has_focus(crate::app::script_view::focus_id()))
                {
                    self.delete_text_selection(ctx)
                } else {
                    self.delete_selected_blocks(ctx)
                }
            }
            ReplaceVoices => crate::app::replace_voices::State::new(&self.project, &self.catalog)
                .map(|state| {
                    self.popup = None;
                    self.voice_replacement = Some(state);
                }),
            // These commands do not yet have editor services.
            CheckForUpdates | Help => Err(format!(
                "{}: {}",
                self.tr("", command.label()),
                self.tr("利用できません", "Unavailable")
            )),
        };
        if let Err(error) = result {
            if matches!(command, PlayPause | PlayBlock) {
                self.player.pause();
                self.transport = crate::app::transport::Transport::Stopped;
            }
            self.error = Some(error);
        }
        ctx.request_repaint();
    }

    pub(crate) fn voice_names(&self) -> Vec<String> {
        self.catalog
            .voices
            .iter()
            .map(|voice| voice.key.clone())
            .collect()
    }

    fn move_block(&mut self, forward: bool) {
        let len = self.project.blocks.len();
        if len == 0 {
            self.selection.clear();
            return;
        }
        let Some(current) = self.selected() else {
            return;
        };
        let adjacent = if forward {
            current.checked_add(1)
        } else {
            current.checked_sub(1)
        };
        let Some(index) = adjacent.filter(|index| *index < len) else {
            return;
        };
        self.select_only(index);
        self.scroll = (index as f32 * 102.0 - 24.0).max(0.0);
        self.playhead = self.project.blocks[index].timestamp;
        self.player.pause();
        self.transport = crate::app::transport::Transport::Paused;
        self.player.full_range();
        self.player.seek(self.playhead);
    }

    pub(crate) fn split_text_block(
        &mut self,
        index: usize,
        caret: usize,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if self.project.blocks.len() >= 10_000 {
            return Err("Block count limit".into());
        }
        self.begin_edit();
        let id = self.project.next_id();
        let left = self
            .project
            .blocks
            .get_mut(index)
            .ok_or("Select an editor block")?;
        let right = left.split_at(id, caret)?;
        self.project.blocks.insert(index + 1, right);
        self.select_only(index + 1);
        let editor = egui::Id::new(("block-text", id));
        let mut state = egui::TextEdit::load_state(ctx, editor).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(0),
            )));
        state.store(ctx, editor);
        ctx.memory_mut(|memory| memory.request_focus(editor));
        Ok(())
    }

    fn text_selection(
        &self,
        ctx: &egui::Context,
    ) -> Result<(usize, egui::Id, egui::text::CCursorRange), String> {
        self.focused_script_selection(ctx)
            .ok_or_else(|| "Focus the block text editor first".into())
    }

    fn text_command(&mut self, command: Command, ctx: &egui::Context) -> Result<(), String> {
        let (index, id, _) = self.text_selection(ctx)?;
        if command == Command::SelectAll {
            let mut state = egui::TextEdit::load_state(ctx, id).unwrap();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(self.project.blocks[index].text.chars().count()),
                )));
            state.store(ctx, id);
            ctx.memory_mut(|m| m.request_focus(id));
            return Ok(());
        }
        self.script_text_command(command, ctx)
    }

    fn delete_text_selection(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let (index, _, range) = self.text_selection(ctx)?;
        let [first, last] = range.sorted_cursors();
        let selection = usize::from(first.index)..usize::from(last.index);
        if selection.is_empty() {
            return Err("Select text to delete".into());
        }
        self.replace_script_text(ctx, index, selection, &[])
    }
}
