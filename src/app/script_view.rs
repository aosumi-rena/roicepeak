use crate::app::{
    document::Block,
    editor::{EditorApp, Popup, script_layout},
    script_playback,
    style::{self, Icon},
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, TextEdit, Vec2};
#[path = "block_rectangle.rs"]
mod rectangle;

pub(crate) fn focus_id() -> egui::Id {
    egui::Id::new("script-surface")
}

impl EditorApp {
    pub(crate) fn blocks(&mut self, ui: &mut egui::Ui, body: Rect) {
        if ui.is_enabled() {
            self.script_clipboard_events(ui.ctx());
            self.script_boundary_keys(ui);
        }
        let right = self.inspector_rect(body).left() - 29.0;
        let bottom = if self.mode.is_some() {
            body.bottom() - 172.0
        } else {
            body.bottom()
        };
        let surface = Rect::from_min_max(body.min, Pos2::new(right + 5.0, bottom));
        let response = ui.interact(surface, focus_id(), Sense::drag());
        if self.project.blocks.is_empty() {
            ui.painter().text(
                surface.center(),
                egui::Align2::CENTER_CENTER,
                self.tr(
                    "クリックしてセリフを入力してください",
                    "Click to enter speech",
                ),
                style::font(15.0),
                style::TEXT,
            );
        }
        let main_focus = response.has_focus() || ui.memory(|memory| memory.focused().is_none());
        let enter = ui.is_enabled()
            && main_focus
            && self.popup.is_none()
            && ui.input_mut(|input| {
                let mut enter = false;
                input.events.retain(|event| {
                    if matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            pressed: true,
                            ..
                        }
                    ) {
                        enter = true;
                        false
                    } else {
                        true
                    }
                });
                enter
            });
        let empty_release = self.project.blocks.is_empty()
            && ui.is_enabled()
            && ui.rect_contains_pointer(surface)
            && ui.input(|input| input.pointer.any_released());
        if enter || empty_release {
            rectangle::cancel(ui.ctx());
            self.append_empty_block(ui.ctx());
        }
        if ui.rect_contains_pointer(surface) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            self.scroll = (self.scroll - scroll).max(0.0);
        }
        let mut content_bottom = body.top() + 24.0;
        self.script_metrics.prepare(
            ui,
            right - body.left() - 59.0,
            self.font_points,
            self.project.blocks.len(),
        );
        let geometries: Vec<_> = self
            .project
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| {
                let geometry = self.script_metrics.geometry(
                    ui,
                    index,
                    Pos2::new(body.left() + 59.0, content_bottom),
                    &block.text,
                );
                content_bottom += geometry.stride;
                geometry
            })
            .collect();
        let max_scroll = if geometries.is_empty() {
            0.0
        } else {
            (content_bottom + 24.0 - surface.bottom()).max(0.0)
        };
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        if let Some(id) = self.playback_guide.reveal.take()
            && let Some(index) = self.project.blocks.iter().position(|block| block.id == id)
        {
            let card = geometries[index]
                .card
                .translate(Vec2::new(0.0, -self.scroll));
            let page = surface.height();
            if page > 0.0 && (card.top() >= surface.bottom() || card.bottom() <= surface.top()) {
                let pages = ((card.top() - surface.top()) / page).floor();
                self.scroll = (self.scroll + pages * page).clamp(0.0, max_scroll);
            }
        }
        if self.transport == crate::app::transport::Transport::Playing
            && !self.playback_pending
            && self.player.loaded()
            && let Some(word) = &self.playback_guide.word
            && let Some(chars) = &word.chars
            && let Some(index) = self
                .project
                .blocks
                .iter()
                .position(|block| block.id == word.block)
        {
            let editor = geometries[index].editor;
            let inset = script_layout::TEXT_MARGIN;
            let galley = script_layout::layout(
                ui,
                &self.project.blocks[index].text,
                editor.width() - f32::from(inset.left) - f32::from(inset.right),
                self.font_points,
                style::TEXT,
            );
            let origin = editor.min + Vec2::new(f32::from(inset.left), f32::from(inset.top));
            if let Some(bounds) = script_playback::word_bounds(&galley, origin, chars.clone()) {
                let minimum_scroll = bounds.bottom() - surface.bottom();
                let maximum_scroll = bounds.top() - surface.top();
                self.scroll = self
                    .scroll
                    .clamp(minimum_scroll.min(maximum_scroll), maximum_scroll)
                    .clamp(0.0, max_scroll);
            }
        }
        let preview = rectangle::update(self, ui, &response, &geometries);
        let strides: Vec<_> = geometries.iter().map(|geometry| geometry.stride).collect();
        let old_clip = ui.clip_rect();
        ui.set_clip_rect(surface.intersect(old_clip));
        let mut remove = false;
        let mut select_voice = false;
        let mut split_text = None;
        let mut script_context = None;
        let mut toggle_loop = false;
        let locale = self.locale;
        let mut block_tops = Vec::with_capacity(self.project.blocks.len());
        for (index, mut geometry) in geometries.into_iter().enumerate() {
            let offset = Vec2::new(0.0, -self.scroll);
            geometry.card = geometry.card.translate(offset);
            geometry.editor = geometry.editor.translate(offset);
            geometry.narrator = geometry.narrator.translate(offset);
            geometry.grip = geometry.grip.translate(offset);
            geometry.delete = geometry.delete.translate(offset);
            geometry.loop_button = geometry.loop_button.translate(offset);
            geometry.timestamp += offset;
            block_tops.push(geometry.card.top());
            let rect = geometry.card;
            if !rect.intersects(surface) {
                continue;
            }
            let block_id = self.project.blocks[index].id;
            let selected = self.selection.contains(block_id);
            let current = self.selection.current() == Some(block_id);
            let previewed = preview.contains(&block_id);
            let active = selected || previewed || ui.rect_contains_pointer(rect);
            let narrator_color = self.narrator_color(&self.project.blocks[index].narrator);
            if active {
                style::card_radius(ui, rect, 8);
                if previewed && !selected {
                    ui.painter()
                        .rect_filled(rect.shrink(1.0), 7, Color32::from_gray(250));
                }
                if current {
                    ui.painter().rect_stroke(
                        rect,
                        8,
                        egui::Stroke::new(1.0, narrator_color),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            let narrator = self.narrator_name(&self.project.blocks[index].narrator);
            let voice_rect = geometry.narrator;
            self.paint_avatar(
                ui,
                &self.project.blocks[index].narrator,
                Rect::from_min_size(voice_rect.min, Vec2::splat(32.0)),
            );
            let select = ui.interact(rect, ui.id().with(("block", block_id)), Sense::click());
            if select.clicked() {
                let modifiers = ui.input(|input| input.modifiers);
                self.click_block(index, modifiers.shift, modifiers.ctrl);
            }
            let block = &mut self.project.blocks[index];
            if active {
                for x in [22.0, 26.0] {
                    for y in [24.0, 29.0, 34.0] {
                        ui.painter().circle_filled(
                            rect.min + Vec2::new(x, y),
                            0.65,
                            Color32::from_gray(165),
                        );
                    }
                }
            }
            let grip = ui.interact(
                geometry.grip,
                ui.id().with(("reorder", block.id)),
                Sense::drag(),
            );
            if grip.drag_started() {
                self.reorder = Some(block.id);
            }
            if grip.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            crate::app::timestamp_popup::button(ui, block, geometry.timestamp, narrator_color);
            let voice = voice_rect;
            style::label(
                ui,
                voice.min + Vec2::new(40.0, 16.0),
                narrator,
                16.0,
                narrator_color,
            );
            style::draw_icon(
                ui,
                voice.right_center() - Vec2::new(8.0, 0.0),
                Icon::Chevron,
                style::TEXT,
            );
            if ui
                .interact(
                    voice,
                    ui.id().with(("block-voice", block.id)),
                    Sense::click(),
                )
                .clicked()
            {
                crate::app::narrator_picker::remember_block_anchor(ui.ctx(), voice, block.id);
                select_voice = true;
            }
            let em = self.font_points;
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                script_layout::layout(ui, text.as_str(), wrap_width, em, style::TEXT)
            };
            // Modified Return inserts a line break.
            let modifiers = ui.input(|input| {
                input
                    .events
                    .iter()
                    .find_map(|event| {
                        if let egui::Event::Key {
                            key: egui::Key::Enter,
                            pressed: true,
                            modifiers,
                            ..
                        } = event
                        {
                            Some(*modifiers)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(input.modifiers)
            });
            let newline = modifiers.shift || modifiers.ctrl || modifiers.alt;
            let selection_fill = ui.visuals().selection.bg_fill;
            ui.visuals_mut().selection.bg_fill = Color32::from_rgba_unmultiplied(
                narrator_color.r(),
                narrator_color.g(),
                narrator_color.b(),
                51,
            );
            let context_capture = crate::app::script_context::capture(ui, block, geometry.editor);
            let edit = ui
                .scope_builder(
                    egui::UiBuilder::new().max_rect(geometry.editor).layout(
                        egui::Layout::centered_and_justified(egui::Direction::TopDown),
                    ),
                    |ui| {
                        TextEdit::multiline(&mut block.text)
                            .id(egui::Id::new(("block-text", block.id)))
                            .frame(egui::Frame::NONE.inner_margin(script_layout::TEXT_MARGIN))
                            .desired_rows(1)
                            .font(style::font(em))
                            .layouter(&mut layouter)
                            .return_key(newline.then_some(egui::KeyboardShortcut::new(
                                modifiers,
                                egui::Key::Enter,
                            )))
                            .hint_text(crate::app::i18n::tr(
                                locale,
                                "クリックしてセリフを入力してください",
                                "Click to enter speech",
                            ))
                            .show(ui)
                    },
                )
                .inner;
            if !edit.response.response.changed()
                && let Some(cursor) = &self.playback_guide.word
                && cursor.block == block_id
                && let Some(chars) = &cursor.chars
            {
                script_playback::highlight(ui.painter(), &edit, chars.clone(), narrator_color);
            }
            let caret = edit
                .cursor_range
                .map(|range| usize::from(range.primary.index));
            let edit = edit.response.response;
            ui.visuals_mut().selection.bg_fill = selection_fill;
            if edit.changed() {
                block.segments.clear();
            }
            let ime_id = egui::Id::new(("block-ime", block.id));
            let mut composing = ui.data_mut(|data| data.get_temp::<bool>(ime_id).unwrap_or(false));
            let was_composing = composing;
            let mut ime_event = false;
            if edit.has_focus() {
                ui.input(|input| {
                    for event in &input.events {
                        match event {
                            egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                                composing = !text.is_empty();
                                ime_event = true;
                            }
                            egui::Event::Ime(egui::ImeEvent::Commit(_)) => {
                                composing = false;
                                ime_event = true;
                            }
                            _ => {}
                        }
                    }
                });
            } else {
                composing = false;
            }
            ui.data_mut(|data| data.insert_temp(ime_id, composing));
            if edit.has_focus()
                && !newline
                && !was_composing
                && !composing
                && !ime_event
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                let id = egui::Id::new(("block-text", block.id));
                if let Some(range) =
                    egui::TextEdit::load_state(ui.ctx(), id).and_then(|s| s.cursor.char_range())
                {
                    split_text = Some((index, usize::from(range.primary.index)));
                }
            }
            if active {
                style::label(
                    ui,
                    Pos2::new(rect.right() - 91.0, rect.bottom() - 25.0),
                    block.text.chars().count().to_string(),
                    11.0,
                    style::TEXT,
                );
            }
            if current
                && style::icon_state(
                    ui,
                    ("block-loop", block.id),
                    geometry.loop_button,
                    Icon::BlockLoop,
                    Color32::from_gray(128),
                    crate::app::i18n::tr(locale, "繰り返し再生", "Toggle loop playback"),
                    self.loop_playback,
                )
                .clicked()
            {
                toggle_loop = true;
            }
            if current
                && style::icon(
                    ui,
                    ("delete", block.id),
                    geometry.delete,
                    Icon::Delete,
                    Color32::from_gray(125),
                    crate::app::i18n::tr(locale, "削除", "Delete"),
                )
                .clicked()
            {
                remove = true;
            }
            if (edit.clicked() || edit.gained_focus()) && !select.clicked() {
                let modifiers = ui.input(|input| input.modifiers);
                self.click_block(index, modifiers.shift, modifiers.ctrl);
                if !modifiers.shift
                    && !modifiers.ctrl
                    && !edit.changed()
                    && !composing
                    && let Some(caret) = caret
                    && let Some(time) = self.playback_guide.text_position(block_id, caret)
                {
                    self.seek_to_word(time);
                    ui.ctx().request_repaint();
                }
            }
            if edit.changed() {
                self.begin_edit();
            }
            if let Some(capture) = context_capture {
                script_context = Some(capture);
            }
        }
        if let Some((index, caret)) = split_text {
            if let Err(error) = self.split_text_block(index, caret, ui.ctx()) {
                self.error = Some(error);
            }
        }
        if remove {
            let removed_height: f32 = self
                .selected_indices()
                .iter()
                .filter_map(|index| strides.get(*index))
                .sum();
            if let Err(error) = self.delete_selected_blocks(ui.ctx()) {
                self.error = Some(error);
            }
            self.scroll = self.scroll.min((max_scroll - removed_height).max(0.0));
        }
        if toggle_loop {
            self.command(crate::app::shortcuts::Command::ToggleLoop, ui.ctx());
        }
        if select_voice {
            self.toggle(Popup::Narrator);
        }
        if ui.input(|i| i.pointer.any_released()) {
            if let (Some(id), Some(pointer)) =
                (self.reorder.take(), ui.input(|i| i.pointer.interact_pos()))
            {
                if let Some(from) = self
                    .project
                    .blocks
                    .iter()
                    .position(|block| block.id == id)
                    .filter(|_| surface.contains(pointer))
                {
                    let to = block_tops
                        .iter()
                        .enumerate()
                        .min_by(|(_, a), (_, b)| {
                            (**a - pointer.y).abs().total_cmp(&(**b - pointer.y).abs())
                        })
                        .map(|(index, _)| index)
                        .unwrap_or(0);
                    let to = to.min(self.project.blocks.len() - 1);
                    if from != to {
                        self.begin_edit();
                        let block = self.project.blocks.remove(from);
                        self.project.blocks.insert(to, block);
                    }
                }
            }
        }
        ui.set_clip_rect(old_clip);
        if let Some(capture) = script_context {
            self.open_script_context(ui.ctx(), capture);
        }
        self.script_context(ui);
    }
    fn append_empty_block(&mut self, ctx: &egui::Context) {
        if self.project.blocks.len() >= 10_000 {
            self.error = Some("Block count limit".into());
            return;
        }
        self.begin_edit();
        let index = self.project.blocks.len();
        let mut block = Block::new(self.project.next_id());
        block.narrator = self.append_narrator(None);
        let id = block.id;
        self.project.blocks.push(block);
        self.select_only(index);
        self.scroll = f32::MAX;
        ctx.memory_mut(|memory| memory.request_focus(egui::Id::new(("block-text", id))));
    }
}
