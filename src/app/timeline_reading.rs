use super::mora::Word;
use crate::app::{document::Block, i18n::Locale, style};
use eframe::egui::{self, Rect, Sense, Vec2};

#[derive(Clone, Default)]
pub(super) struct Editor {
    target: Option<(u64, u64, usize)>,
    text: String,
    composition: super::composition::Composition,
    field_id: Option<egui::Id>,
    rect: Option<Rect>,
}

impl Editor {
    pub(super) fn load(ctx: &egui::Context, block: u64) -> Self {
        let mut value = ctx
            .data_mut(|d| d.get_temp::<Self>(egui::Id::new("reading-editor")))
            .unwrap_or_default();
        if value.target.is_some_and(|target| target.0 != block) {
            value = Self::default();
        }
        value
    }

    pub(super) fn store(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("reading-editor"), self));
    }

    pub(super) fn word(
        &mut self,
        ui: &mut egui::Ui,
        block: &mut Block,
        word: &Word,
        card: Rect,
        basis_current: bool,
        locale: Locale,
        error: &mut Option<String>,
    ) -> bool {
        let mut changed = false;
        if !ui.is_enabled() {
            self.target = None;
            self.field_id = None;
            self.composition = super::composition::Composition::default();
            return false;
        }
        let rect = Rect::from_min_size(
            card.min + Vec2::new(0.0, 28.0),
            Vec2::new(card.width(), 32.0),
        );
        let target = (block.id, word.sentence_id, word.token);
        let id = ui.id().with(("word-reading", target));
        let enabled = basis_current && word.language == 1;
        let mut initial = false;
        if self.target != Some(target) {
            let response = ui.interact(
                rect,
                id.with("open"),
                if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, enabled, &word.text)
            });
            if !response.clicked() {
                return false;
            }
            self.target = Some(target);
            self.text = word.morae.iter().map(|m| m.reading.as_str()).collect();
            self.composition = super::composition::Composition::default();
            initial = true;
            ui.memory_mut(|m| m.request_focus(id));
        }
        self.rect = Some(rect);
        self.field_id = Some(id);
        let focused = initial || ui.memory(|m| m.has_focus(id));
        let (enter, escape) = self.composition.keys(ui, focused);
        ui.painter().rect_filled(rect, 1, egui::Color32::WHITE);
        ui.painter().rect_stroke(
            rect,
            1,
            egui::Stroke::new(1.0, style::BORDER),
            egui::StrokeKind::Inside,
        );
        let field = ui.put(
            rect.shrink2(Vec2::new(3.0, 1.0)),
            egui::TextEdit::singleline(&mut self.text)
                .id(id)
                .char_limit(4096)
                .font(style::font(16.0))
                .horizontal_align(egui::Align::Center)
                .vertical_align(egui::Align::Center)
                .margin(egui::Margin::ZERO)
                .frame(egui::Frame::NONE),
        );
        if initial {
            field.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(self.text.chars().count()),
                    )));
                state.store(ui.ctx(), id);
            }
        }
        if escape
            || self
                .composition
                .finish(enter || (!initial && field.lost_focus()))
        {
            if !escape && basis_current {
                match self.commit(block) {
                    Ok(value) => changed = value,
                    Err(_) => {
                        *error = Some(format!(
                            "{}\n{}",
                            crate::app::i18n::tr(locale, "読みの編集", "Edit Pronunciation"),
                            crate::app::i18n::tr(
                                locale,
                                "読みは仮名で入力してください。",
                                "Please input a pronunciation using kanas."
                            )
                        ));
                    }
                }
            }
            self.target = None;
            self.field_id = None;
            self.composition = super::composition::Composition::default();
            if field.has_focus() {
                ui.memory_mut(|m| m.surrender_focus(id));
            }
        }
        changed
    }

    fn commit(&self, block: &mut Block) -> Result<bool, String> {
        let (block_id, sentence_id, token_index) = self.target.ok_or("No edited word")?;
        if block.id != block_id {
            return Err("Edited block changed".into());
        }
        let sentence = block
            .sentences
            .iter_mut()
            .find(|s| s.id == sentence_id)
            .ok_or("Sentence changed")?;
        if sentence.query.text != sentence.text
            || !sentence
                .words
                .get(token_index)
                .zip(sentence.query.controlled.get(token_index))
                .is_some_and(|(word, query)| word.matches_query(query))
        {
            return Err("Reading query basis no longer matches the word".into());
        }
        let baseline = sentence
            .query
            .raw
            .get(token_index)
            .ok_or("Reading basis unavailable")?
            .clone();
        let word = sentence.words.get_mut(token_index).ok_or("Word changed")?;
        let syllables = roicepeak::japanese::edit_reading(&self.text).map_err(|e| e.to_string())?;
        let token = roicepeak::phonemes::Token {
            text: word.surface.clone(),
            language: word.language,
            pos: word.pos,
            syllables,
        };
        let changed = word.replace_reading(&baseline, &token);
        if changed {
            block.segments.clear();
        }
        Ok(changed)
    }
}

pub(super) fn finish_pointer_edit(
    ctx: &egui::Context,
    project: &mut crate::app::document::Project,
    selected: Option<u64>,
    basis_current: bool,
    visible: bool,
    ui_enabled: bool,
    locale: Locale,
    error: &mut Option<String>,
) -> bool {
    let mut changed = false;
    let Some(mut editor) = ctx.data_mut(|d| d.get_temp::<Editor>(egui::Id::new("reading-editor")))
    else {
        return false;
    };
    let Some(target) = editor.target else {
        return false;
    };
    if !ui_enabled {
        editor.target = None;
        editor.field_id = None;
        editor.composition = super::composition::Composition::default();
        editor.store(ctx);
        return false;
    }
    editor.composition.refresh(ctx, editor.field_id, true);
    let outside = ctx.input(|input| {
        input.pointer.any_pressed()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|p| editor.rect.is_some_and(|r| !r.contains(p)))
    });
    if editor
        .composition
        .finish(outside || selected != Some(target.0) || !visible)
    {
        if basis_current {
            if let Some(block) = project.blocks.iter_mut().find(|b| b.id == target.0) {
                match editor.commit(block) {
                    Ok(value) => changed = value,
                    Err(_) => {
                        *error = Some(
                            crate::app::i18n::tr(
                                locale,
                                "読みは仮名で入力してください。",
                                "Please input a pronunciation using kanas.",
                            )
                            .into(),
                        );
                    }
                }
            }
        }
        editor.target = None;
        editor.field_id = None;
        editor.composition = super::composition::Composition::default();
    }
    editor.store(ctx);
    changed
}
