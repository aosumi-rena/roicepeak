use crate::app::{editor::EditorApp, settings::Preferences};
use eframe::egui;

impl EditorApp {
    pub(crate) fn restore_settings(&mut self) {
        let preferences = &self.resources.editor;
        self.font_points = preferences.font_points;
        self.output_id = preferences.output_id.clone();
        self.global_volume = preferences.global_volume;
        self.loop_playback = preferences.loop_playback;
        self.mode = preferences.mode;
        self.settings_state.loaded(&self.resources);
    }

    fn capture_settings(&mut self, ctx: Option<&egui::Context>, visible: bool) {
        let mut window = self.resources.editor.window.clone();
        if let Some(ctx) = ctx {
            self.settings_state
                .observe_window(&mut window, ctx, visible);
        }
        self.resources.editor = Preferences {
            font_points: self.font_points,
            output_id: self.output_id.clone(),
            global_volume: self.global_volume,
            loop_playback: self.loop_playback,
            mode: self.mode,
            window,
        };
    }

    pub(crate) fn synchronize_settings(&mut self, ctx: &egui::Context, visible: bool) {
        self.capture_settings(Some(ctx), visible);
        let closing = ctx.input(|input| input.viewport().close_requested());
        let result = if closing {
            self.settings_state.flush(&self.resources)
        } else {
            self.settings_state.synchronize(&self.resources, ctx)
        };
        if let Err(error) = result {
            self.error = Some(error);
            if closing {
                self.quit_approved = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
    }

    pub(crate) fn flush_settings(&mut self) {
        self.capture_settings(None, false);
        let mut locations = self.resources.clone();
        if let Some(draft) = &self.resource_draft {
            locations.voice_roots = draft.voice_roots.clone();
            locations.dictionary = draft.dictionary.clone();
            locations.show_portrait = draft.show_portrait;
        }
        if let Err(error) = locations
            .validate()
            .and_then(|()| self.settings_state.flush(&locations))
        {
            self.error = Some(error);
        }
    }
}
