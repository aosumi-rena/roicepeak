#[path = "script_layout.rs"]
pub(crate) mod script_layout;
use crate::app::{document::Project, style};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Accent,
    Intonation,
    Duration,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Popup {
    File,
    Speed,
    Pitch,
    Pause,
    Emotion,
    Narrator,
    EmotionPreset,
    Volume,
}
#[derive(Clone)]
pub enum PendingAction {
    New,
    Open,
    OpenPath(PathBuf),
    ImportSrt,
    ImportSsml,
    Quit,
}

pub struct EditorApp {
    pub catalog: crate::app::catalog::Catalog,
    pub portrait_state: crate::app::portrait::State,
    pub avatar_cache: std::collections::BTreeMap<String, std::sync::Arc<[u8]>>,
    pub resources: crate::app::resources::Locations,
    pub resource_settings: bool,
    pub resource_draft: Option<crate::app::resources::Locations>,
    pub transport: crate::app::transport::Transport,
    pub playback_scope: crate::app::transport::Scope,
    pub audio_commands: Vec<crate::app::shortcuts::Command>,
    pub dispatching_audio: bool,
    pub project: Project,
    pub(crate) selection: crate::app::block_selection::BlockSelection,
    pub locale: crate::app::i18n::Locale,
    pub mode: Option<Mode>,
    pub popup: Option<Popup>,
    pub preferences: bool,
    pub experimental_settings: bool,
    pub voice_replacement: Option<crate::app::replace_voices::State>,
    pub export: crate::app::export::State,
    pub dictionary: bool,
    pub about: bool,
    pub error: Option<String>,
    pub filename: Option<PathBuf>,
    pub(crate) recent_projects: crate::app::recent_projects::RecentProjects,
    pub dirty: bool,
    pub(crate) undo: Vec<crate::app::edit_history::Entry>,
    pub(crate) redo: Vec<crate::app::edit_history::Entry>,
    pub(crate) history_selection: Option<crate::app::block_selection::BlockSelection>,
    pub(crate) phoneme_history: crate::app::phoneme_history::State,
    pub font_points: f32,
    pub dictionary_ui: crate::app::dictionary::Editor,
    pub(crate) dictionary_state: crate::app::dictionary::State,
    pub playhead: f64,
    pub global_volume: f32,
    pub scroll: f32,
    pub reorder: Option<u64>,
    pub history_navigation: bool,
    pub pending: Option<PendingAction>,
    pub(crate) native_save: Option<crate::app::native_save::Pending>,
    pub quit_approved: bool,
    pub player: crate::app::audio::Player,
    pub rendered_audio: std::sync::Arc<Vec<f32>>,
    pub output_id: String,
    pub audio_outputs: Vec<crate::app::audio::Output>,
    pub loop_playback: bool,
    pub shortcuts: crate::app::shortcuts::ShortcutMap,
    pub shortcut_settings: bool,
    pub ime_composing: bool,
    pub shortcut_dialog: crate::app::shortcut_dialog::State,
    pub find_replace: crate::app::find_replace::FindReplace,
    pub render_config: Option<crate::app::render::Config>,
    pub query_basis_current: bool,
    pub render_worker: crate::app::render::Worker,
    pub render_busy: bool,
    pub render_status: String,
    pub playback_generation: u64,
    pub playback: Option<std::sync::Arc<crate::app::render::Playback>>,
    pub playback_pending: bool,
    pub rendered_blocks: std::collections::BTreeMap<u64, (f64, f64)>,
}
impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.clear();
        fonts.font_data.insert(
            "noto-cjk".into(),
            std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
                "../../assets/NotoSansCJKjp-Regular.otf"
            ))),
        );
        fonts
            .families
            .insert(egui::FontFamily::Proportional, vec!["noto-cjk".into()]);
        fonts
            .families
            .insert(egui::FontFamily::Monospace, vec!["noto-cjk".into()]);
        let locale = crate::app::i18n::Locale::default();
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        let mut style = (*cc.egui_ctx.style_of(egui::Theme::Light)).clone();
        style.visuals.override_text_color = Some(style::TEXT);
        style.visuals.window_fill = Color32::WHITE;
        style.visuals.panel_fill = style::BACKGROUND;
        style.visuals.selection.bg_fill = Color32::from_gray(224);
        style
            .text_styles
            .insert(egui::TextStyle::Body, style::font(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, style::font(14.0));
        cc.egui_ctx.set_style_of(egui::Theme::Light, style);
        let mut editor = Self {
            catalog: crate::app::catalog::Catalog::default(),
            avatar_cache: Default::default(),
            portrait_state: Default::default(),
            resources: crate::app::resources::Locations::default(),
            resource_settings: false,
            resource_draft: None,
            transport: crate::app::transport::Transport::Stopped,
            playback_scope: crate::app::transport::Scope::Project,
            audio_commands: Vec::new(),
            dispatching_audio: false,
            project: Project::default(),
            selection: crate::app::block_selection::BlockSelection::only(1),
            locale,
            mode: None,
            popup: None,
            preferences: false,
            experimental_settings: false,
            voice_replacement: None,
            export: crate::app::export::State::default(),
            dictionary: false,
            about: false,
            error: None,
            filename: None,
            recent_projects: Default::default(),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            history_selection: None,
            phoneme_history: Default::default(),
            font_points: script_layout::DEFAULT_EM,
            dictionary_ui: crate::app::dictionary::Editor::default(),
            dictionary_state: crate::app::dictionary::State::load(),
            playhead: 0.0,
            global_volume: 100.0,
            scroll: 0.0,
            reorder: None,
            history_navigation: false,
            pending: None,
            native_save: None,
            quit_approved: false,
            player: crate::app::audio::Player::default(),
            rendered_audio: std::sync::Arc::default(),
            output_id: String::new(),
            audio_outputs: crate::app::audio::outputs().unwrap_or_default(),
            loop_playback: false,
            shortcuts: crate::app::shortcuts::ShortcutMap::defaults(),
            shortcut_settings: false,
            ime_composing: false,
            shortcut_dialog: crate::app::shortcut_dialog::State::default(),
            find_replace: crate::app::find_replace::FindReplace::default(),
            render_config: None,
            query_basis_current: false,
            render_worker: crate::app::render::Worker::default(),
            render_busy: false,
            render_status: String::new(),
            playback_generation: 0,
            playback: None,
            playback_pending: false,
            rendered_blocks: std::collections::BTreeMap::new(),
        };
        if let Err(error) = editor.dictionary_snapshot() {
            editor.error = Some(error);
        }
        match crate::app::recent_projects::RecentProjects::load() {
            Ok(recent) => editor.recent_projects = recent,
            Err(error) => editor.error = Some(error),
        }
        match crate::app::shortcuts::ShortcutMap::load_local() {
            Ok(shortcuts) => editor.shortcuts = shortcuts,
            Err(error) => editor.error = Some(error),
        }
        match crate::app::resources::Locations::load() {
            Ok(locations) => {
                editor.locale = locations.locale;
                editor.resources = locations;
                if let Err(error) = editor.reload_voices() {
                    editor.error = Some(error);
                }
            }
            Err(error) => editor.error = Some(error),
        }
        editor.project.blocks[0].narrator = editor.default_narrator();
        editor.project.export_settings =
            editor.resources.export_settings.clone().unwrap_or_default();
        editor
    }
    pub fn tr<'a>(&self, ja: &'a str, en: &'a str) -> &'a str {
        crate::app::i18n::tr(self.locale, ja, en)
    }
    pub fn set_locale(&mut self, locale: crate::app::i18n::Locale) {
        self.locale = locale;
        self.resources.locale = locale;
        if let Some(draft) = &mut self.resource_draft {
            draft.locale = locale;
        }
        if let Err(error) = self.resources.save() {
            self.error = Some(error);
        }
    }
    pub fn toggle(&mut self, popup: Popup) {
        self.popup = if self.popup == Some(popup) {
            None
        } else {
            Some(popup)
        };
    }
    pub fn invalidate_audio(&mut self) {
        self.transport = crate::app::transport::Transport::Stopped;
        self.render_worker.cancel();
        self.render_busy = false;
        self.player.stop();
        self.playback = None;
        self.playback_pending = false;
        self.rendered_audio = std::sync::Arc::default();
        self.rendered_blocks.clear();
        self.render_status.clear();
    }
    pub(crate) fn poll_render(&mut self, ctx: &egui::Context) {
        if self.render_busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        }
        let Some(result) = self.render_worker.poll() else {
            return;
        };
        self.render_busy = false;
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.transport = crate::app::transport::Transport::Stopped;
                self.render_status = "Synthesis failed".into();
                self.error = Some(error);
                return;
            }
        };
        self.playback_generation = result.profile.generation;
        if !result.source.same_content(&self.project)
            || !self.dictionary_state.matches(&result.user_dictionary)
        {
            self.transport = crate::app::transport::Transport::Stopped;
            self.render_status = "Discarded synthesis for an older project state".into();
            return;
        }
        let applied = (|| -> Result<(), String> {
            if result.rate != 48_000
                || result.pcm.is_empty()
                || result.pcm.len() > 48_000 * 60 * 60
                || result.pcm.iter().any(|v| !v.is_finite())
            {
                return Err("Invalid synthesis audio result".into());
            }
            let mut updated = self.project.clone();
            let mut ranges = std::collections::BTreeMap::new();
            let mut seen = std::collections::BTreeSet::new();
            let duration = result.pcm.len() as f64 / result.rate as f64;
            for rendered in result.blocks {
                let end = rendered.start + rendered.duration;
                if !rendered.start.is_finite()
                    || !end.is_finite()
                    || rendered.start < 0.0
                    || rendered.duration < 0.0
                    || end > duration + 1.0 / result.rate as f64
                    || !seen.insert(rendered.id)
                {
                    return Err("Invalid synthesis block range".into());
                }
                if rendered.duration > 0.0 {
                    ranges.insert(rendered.id, (rendered.start, end.min(duration)));
                }
                let block = updated
                    .blocks
                    .iter_mut()
                    .find(|b| b.id == rendered.id)
                    .ok_or("Unknown synthesis block ID")?;
                block.timestamp = rendered.start;
                block.segments = rendered.segments;
                block.sentences = rendered.sentences;
                block.sentence_serial = block
                    .sentence_serial
                    .max(block.sentences.iter().map(|s| s.id).max().unwrap_or(0));
            }
            if seen.len() != updated.blocks.len() {
                return Err("Incomplete synthesis block metadata".into());
            }
            updated.validate()?;
            self.dirty |= updated != self.project;
            self.project = updated;
            self.query_basis_current = true;
            self.rendered_audio = result.pcm;
            self.rendered_blocks = ranges;
            self.playhead = self.playhead.min(duration);
            self.render_status = format!(
                "Ready: {} blocks, {:.2} seconds",
                self.rendered_blocks.len(),
                duration
            );
            Ok(())
        })();
        if let Err(error) = applied {
            self.transport = crate::app::transport::Transport::Stopped;
            self.render_status = "Synthesis result rejected".into();
            self.error = Some(error);
        } else {
            if self.transport == crate::app::transport::Transport::Playing
                && self.playback.is_none()
            {
                if let Err(error) = self.resume_prepared_playback() {
                    self.transport = crate::app::transport::Transport::Paused;
                    self.error = Some(error);
                }
            }
        }
    }
}

impl eframe::App for EditorApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        crate::app::portrait::raw_input(ctx, input);
        crate::app::inspector_scroll::raw_input(ctx, input);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.history_navigation = false;
        self.history_selection = None;
        let context = ui.ctx().clone();
        crate::app::phoneme_history::begin_frame(&context);
        style::set_value_policy(&context, self.resources.value_policy());
        self.poll_preparation(&context);
        self.poll_playback(&context);
        self.poll_render(&context);
        if let Some(timing) = self.player.poll_output_timing() {
            self.render_worker.record_output_timing(timing);
        }
        self.synchronize_transport();
        let before = self.project.clone();
        let selection_before = self.selection.clone();
        self.finish_reading_edit(&context);
        self.player.gain(self.global_volume / 100.0);
        if self.player.loaded() {
            self.playhead = self.player.position();
        }
        if self.player.is_playing() {
            context.request_repaint_after(std::time::Duration::from_millis(16));
        }
        if let Some(error) = self.player.take_error() {
            self.transport = crate::app::transport::Transport::Paused;
            self.error = Some(error);
        }
        if context.input(|i| i.viewport().close_requested())
            && (self.dirty || self.native_save.is_some())
            && !self.quit_approved
        {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if let Some(save) = &mut self.native_save {
                save.after_save = Some(PendingAction::Quit);
            } else {
                self.pending = Some(PendingAction::Quit);
            }
        }
        self.dispatch_shortcuts(&context);
        self.isolate_alert_focus(&context);
        let tooltip_scope = crate::app::root_tooltip::Scope::new(&context);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(style::BAR))
            .show(ui, |ui| {
                let rect = ui.max_rect();
                if self.pending.is_some()
                    || self.error.is_some()
                    || self.about
                    || self.preferences
                    || self.dictionary
                    || self.export.active()
                    || self.resource_settings
                    || self.experimental_settings
                    || self.shortcut_settings
                    || self.voice_replacement.is_some()
                    || crate::app::emotion_presets::active(&context)
                {
                    ui.disable();
                }
                self.toolbar(ui, rect);
                let body = Rect::from_min_max(rect.min + Vec2::new(0.0, 44.0), rect.max);
                ui.painter().rect_filled(
                    body,
                    egui::CornerRadius {
                        nw: 12,
                        ne: 12,
                        sw: 0,
                        se: 0,
                    },
                    style::BACKGROUND,
                );
                self.blocks(ui, body);
                self.inspector(ui, body);
                if self.resources.show_portrait {
                    let panel = Rect::from_min_size(
                        Pos2::new(body.right() - crate::app::portrait::PANEL_WIDTH, body.top()),
                        Vec2::new(crate::app::portrait::PANEL_WIDTH, body.height()),
                    );
                    if crate::app::portrait::show(self, ui, panel) {
                        self.set_portrait_visible(false);
                    }
                }
                self.timeline(ui, body);
                self.popups(ui, &context, rect);
            });
        drop(tooltip_scope);
        self.dialogs(&context);
        if self.export.active() {
            let names = self
                .catalog
                .voices
                .iter()
                .map(|v| (v.name.clone(), v.name.clone()))
                .collect();
            let user_dictionary = self.dictionary_snapshot();
            self.export.show(
                &context,
                &self.project,
                self.render_config.as_ref(),
                user_dictionary,
                &names,
                self.locale,
            );
        }
        let _ = self.export.take_completed();
        self.resource_window(&context);
        self.experimental_window(&context);
        self.replace_voices_window(&context);
        self.shortcut_window(&context);
        if self.find_replace.open {
            self.begin_edit();
        }
        let mut find_current = self.selected();
        if let Some(found) =
            self.find_replace
                .show(&context, &mut self.project, &mut find_current, self.locale)
        {
            self.select_only(found.block);
            let rect = ui.max_rect();
            let body_width = self.inspector_rect(rect).left() - 29.0 - rect.left() - 59.0;
            self.scroll = self
                .project
                .blocks
                .iter()
                .take(found.block)
                .map(|block| {
                    script_layout::Geometry::new(
                        ui,
                        Pos2::ZERO,
                        body_width,
                        &block.text,
                        self.font_points,
                    )
                    .stride
                })
                .sum::<f32>()
                .max(0.0);
            if let Some(block) = self.project.blocks.get(found.block) {
                let id = egui::Id::new(("block-text", block.id));
                if let Some(mut state) = egui::TextEdit::load_state(&context, id) {
                    let start = block.text[..found.start].chars().count();
                    let end = block.text[..found.end].chars().count();
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(start),
                            egui::text::CCursor::new(end),
                        )));
                    state.store(&context, id);
                }
            }
        }
        if !self.history_navigation {
            if let Err(error) = self.project.commit_edit_state(&before) {
                self.project = before.clone();
                self.selection.prune(&self.project);
                self.error = Some(error);
            }
        }
        let selection = self.history_selection.take().unwrap_or(selection_before);
        self.record_history_frame(&context, before, selection);
        // Commit this frame's text/history before submitting synthesis.
        self.dispatching_audio = true;
        for command in std::mem::take(&mut self.audio_commands) {
            self.command(command, &context);
        }
        self.dispatching_audio = false;
        if let Some(settings) = self.export.take_accepted() {
            self.project.export_settings = settings.clone();
            self.resources.export_settings = Some(settings.clone());
            if let Some(draft) = &mut self.resource_draft {
                draft.export_settings = Some(settings);
            }
            if let Err(error) = self.resources.save() {
                self.error = Some(error);
            }
        }
        self.poll_native_save(&context);
        let tooltip_enabled = self.pending.is_none()
            && self.error.is_none()
            && !self.about
            && !self.preferences
            && !self.dictionary
            && !self.export.active()
            && !self.resource_settings
            && !self.experimental_settings
            && !self.shortcut_settings
            && self.voice_replacement.is_none()
            && !crate::app::emotion_presets::active(&context);
        let root_rect = ui.max_rect();
        crate::app::root_tooltip::paint(ui, root_rect, tooltip_enabled);
        context.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "ROICEPEAK{}{}",
            self.filename
                .as_ref()
                .map(|p| format!(" — {}", p.file_name().unwrap_or_default().to_string_lossy()))
                .unwrap_or_default(),
            if self.dirty { " *" } else { "" }
        )));
    }
}
