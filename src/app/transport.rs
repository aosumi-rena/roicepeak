//! Playback intent is independent of sentence synthesis readiness.
use crate::app::{editor::EditorApp, shortcuts::Command};
use eframe::egui;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Transport {
    #[default]
    Stopped,
    Playing,
    Paused,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    Project,
    Block(u64),
}

impl EditorApp {
    pub(crate) fn rebind_output(&mut self) -> Result<(), String> {
        if !self.player.loaded() {
            return Ok(());
        }
        let (start, end) = self
            .player
            .range_seconds()
            .ok_or("Current playback range is unavailable")?;
        self.player.pause();
        self.playhead = self.player.position();
        self.player.stop();
        let _ = self.player.take_error();
        let result = (|| {
            self.ensure_player()?;
            self.player
                .set_range(start, end.min(self.player.duration()))?;
            self.player.seek(self.playhead);
            self.player.repeat(self.loop_playback);
            self.player.gain(self.global_volume / 100.0);
            if self.transport == Transport::Playing {
                self.player.resume();
            }
            Ok(())
        })();
        if result.is_err() {
            self.player.stop();
            self.transport = Transport::Paused;
        }
        result
    }

    pub(crate) fn play_pause(&mut self) -> Result<(), String> {
        self.synchronize_transport();
        if self.transport == Transport::Playing {
            self.pause_transport();
            return Ok(());
        }
        self.playback_scope = Scope::Project;
        self.transport = Transport::Playing;
        self.prepare_playback(Command::PlayPause)
    }

    pub(crate) fn play_current_block(&mut self) -> Result<(), String> {
        self.synchronize_transport();
        if self.transport == Transport::Playing {
            self.pause_transport();
            return Ok(());
        }
        let block = self
            .selected()
            .and_then(|index| self.project.blocks.get(index))
            .ok_or("Select a block first")?;
        self.playback_scope = Scope::Block(block.id);
        self.playhead = self
            .rendered_blocks
            .get(&block.id)
            .map_or(block.timestamp, |range| range.0);
        self.transport = Transport::Playing;
        self.prepare_playback(Command::PlayBlock)
    }

    fn pause_transport(&mut self) {
        self.player.pause();
        if self.player.loaded() {
            self.playhead = self.player.position();
        }
        self.transport = Transport::Paused;
        // Keep pending synthesis while paused.
    }

    fn prepare_playback(&mut self, _command: Command) -> Result<(), String> {
        if self
            .playback
            .as_ref()
            .is_some_and(|source| source.failure().is_some() || source.cancelled())
        {
            self.player.stop();
            self.playback = None;
        }
        let result = if self.playback_pending {
            Ok(())
        } else if self.playback.is_none() && self.rendered_audio.is_empty() {
            let config = self.render_config.clone().ok_or_else(|| {
                "No voices are available. Select a voice resource folder in settings.".to_owned()
            });
            config.and_then(|config| {
                let request = crate::app::render::Request {
                    config,
                    project: self.project.clone(),
                    user_dictionary: self.dictionary_snapshot()?,
                };
                match self.playback_scope {
                    Scope::Project => self.render_worker.start_playback(request, self.playhead)?,
                    Scope::Block(id) => self.render_worker.start_block_playback(request, id)?,
                }
                self.playback_pending = true;
                self.render_status = "Synthesizing…".into();
                Ok(())
            })
        } else {
            self.resume_prepared_playback()
        };
        if result.is_err() {
            self.transport = Transport::Stopped;
        }
        result
    }

    /// Called after a completed render is installed. A paused request stays paused.
    pub(crate) fn resume_prepared_playback(&mut self) -> Result<(), String> {
        if self.transport != Transport::Playing
            || self.playback_pending
            || (self.playback.is_none() && self.render_busy)
        {
            return Ok(());
        }
        if let Scope::Block(id) = self.playback_scope {
            let (start, end) = self.block_audio_range_for(id)?;
            if !(start..end).contains(&self.playhead) {
                self.playhead = start;
            }
        }
        self.ensure_player()?;
        match self.playback_scope {
            Scope::Project => {
                self.player.full_range();
                self.player
                    .seek(if self.playhead >= self.player.duration() {
                        0.0
                    } else {
                        self.playhead
                    });
            }
            Scope::Block(id) => {
                let (start, end) = self.block_audio_range_for(id)?;
                self.player.set_range(start, end)?;
                self.playhead = if (start..end).contains(&self.playhead) {
                    self.playhead
                } else {
                    start
                };
                self.player.seek(self.playhead);
            }
        }
        self.player.resume();
        Ok(())
    }

    /// Observe completion of an already started device stream, never render readiness.
    pub(crate) fn synchronize_transport(&mut self) {
        if self.playback.is_some() {
            if self.transport == Transport::Playing && self.player.ended() {
                self.playhead = self.player.position();
                if self.loop_playback {
                    self.player.rewind_range();
                    self.player.resume();
                } else {
                    self.transport = Transport::Stopped;
                }
            }
            return;
        }
        if self.transport == Transport::Playing
            && !self.render_busy
            && self.player.loaded()
            && !self.player.is_playing()
        {
            self.playhead = self.player.position();
            self.transport = Transport::Stopped;
        }
    }

    fn ensure_player(&mut self) -> Result<(), String> {
        if let Some(source) = &self.playback {
            if self.player.playback_generation() != Some(source.generation) {
                self.player.load_playback(
                    std::sync::Arc::clone(source),
                    &self.output_id,
                    self.playhead,
                )?;
                self.player.gain(self.global_volume / 100.0);
            }
            return Ok(());
        }
        if self.rendered_audio.is_empty() {
            return Err("No synthesized speech is available".into());
        }
        if !self.player.loaded() {
            self.player.load_for_generation(
                self.rendered_audio.clone(),
                48_000,
                &self.output_id,
                self.playback_generation,
            )?;
            self.player.repeat(self.loop_playback);
            self.player.gain(self.global_volume / 100.0);
        }
        Ok(())
    }

    pub(crate) fn block_audio_range_for(&self, id: u64) -> Result<(f64, f64), String> {
        let &(start, end) = self
            .rendered_blocks
            .get(&id)
            .ok_or("The selected block has no associated rendered audio")?;
        let available = self.playback_duration();
        if !start.is_finite()
            || !end.is_finite()
            || start < 0.0
            || start >= end
            || start >= available
            || end > available + 1.0 / 48_000.0
        {
            return Err("The selected block range is not covered by rendered audio".into());
        }
        // Clamp the block end to the available sample duration.
        Ok((start, end.min(available)))
    }
    pub(crate) fn playback_duration(&self) -> f64 {
        self.playback
            .as_ref()
            .map_or(self.rendered_audio.len() as f64 / 48_000.0, |source| {
                source.duration()
            })
    }
    pub(crate) fn poll_playback(&mut self, ctx: &egui::Context) {
        if self.playback_pending {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        }
        if let Some(result) = self.render_worker.poll_playback() {
            self.playback_pending = false;
            let applied = result.and_then(|source| {
                let dictionary = self.dictionary_snapshot()?;
                let updated = match crate::app::preparation_ui::apply(
                    &self.project,
                    &dictionary,
                    &source.plan,
                ) {
                    Ok(updated) => updated,
                    Err(error) => {
                        source.cancel();
                        return Err(error);
                    }
                };
                let duration = source.duration();
                self.rendered_blocks = source
                    .plan
                    .blocks
                    .iter()
                    .filter(|block| block.duration > 0.0)
                    .map(|block| {
                        (
                            block.id,
                            (block.start, (block.start + block.duration).min(duration)),
                        )
                    })
                    .collect();
                self.dirty |= updated != self.project;
                self.project = updated;
                self.query_basis_current = true;
                self.playback_generation = source.generation;
                self.playhead = self.playhead.min(duration);
                self.playback = Some(source);
                self.resume_prepared_playback()
            });
            if let Err(error) = applied {
                self.transport = Transport::Stopped;
                self.error = Some(error);
            }
        }
        if let Some(source) = &self.playback {
            if let Some(error) = source.take_error() {
                self.player.pause();
                self.transport = Transport::Paused;
                self.error = Some(error);
            } else if self.transport == Transport::Playing {
                self.render_status = if self.player.buffering() {
                    "Synthesizing…"
                } else {
                    "Ready"
                }
                .into();
            }
        }
    }
}
