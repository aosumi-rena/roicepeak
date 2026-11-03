use crate::app::editor::EditorApp;
use eframe::egui;

impl EditorApp {
    pub(crate) fn delete_selected_blocks(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let indices = self.selected_indices();
        let Some(&first) = indices.first() else {
            return Err("Select a block or text to delete".into());
        };
        self.begin_edit();
        if self.project.blocks.len() == 1 {
            let block = &mut self.project.blocks[0];
            block.set_text(String::new())?;
            let id = egui::Id::new(("block-text", block.id));
            self.selection.select_only(block.id);
            self.scroll = 0.0;
            let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(0),
                )));
            state.store(ctx, id);
            ctx.memory_mut(|memory| memory.request_focus(id));
        } else {
            let ids: std::collections::BTreeSet<_> = self.selected_ids().into_iter().collect();
            self.project.blocks.retain(|block| !ids.contains(&block.id));
            self.selection.clear();
            if !self.project.blocks.is_empty() {
                self.select_only(first.min(self.project.blocks.len() - 1));
            }
        }
        Ok(())
    }
}
