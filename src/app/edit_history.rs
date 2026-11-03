use crate::app::{block_selection::BlockSelection, document::Project, editor::EditorApp};

pub(crate) struct Entry {
    pub project: Project,
    pub selection: BlockSelection,
}

impl EditorApp {
    pub(crate) fn selected(&self) -> Option<usize> {
        self.selection.current_index(&self.project)
    }

    pub(crate) fn selected_indices(&self) -> Vec<usize> {
        self.selection.indices(&self.project)
    }

    pub(crate) fn selected_ids(&self) -> Vec<u64> {
        self.selection.ids(&self.project)
    }

    pub(crate) fn select_only(&mut self, index: usize) {
        if let Some(block) = self.project.blocks.get(index) {
            self.selection.select_only(block.id);
        }
    }

    pub(crate) fn click_block(&mut self, index: usize, shift: bool, control: bool) {
        self.selection.click(&self.project, index, shift, control);
    }

    pub(crate) fn voice_target_indices(&self, clicked: Option<u64>) -> Vec<usize> {
        match clicked {
            Some(id) if !self.selection.contains(id) => self
                .project
                .blocks
                .iter()
                .position(|block| block.id == id)
                .into_iter()
                .collect(),
            _ => self.selected_indices(),
        }
    }

    pub(crate) fn begin_edit(&mut self) {
        if self.history_selection.is_none() {
            self.history_selection = Some(self.selection.clone());
        }
    }

    pub(crate) fn push_history(&mut self, previous: Project, selection: BlockSelection) {
        self.content_changed(&previous);
        self.store_undo(Entry {
            project: previous,
            selection,
        });
    }

    pub(crate) fn content_changed(&mut self, previous: &Project) {
        self.query_basis_current &= crate::app::query_basis::matches(previous, &self.project);
        self.invalidate_audio();
        self.dirty = true;
        self.prepare_project();
    }

    pub(crate) fn store_undo(&mut self, entry: Entry) {
        self.undo.push(entry);
        if self.undo.len() > 128 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub(crate) fn undo(&mut self, ctx: &eframe::egui::Context) {
        self.finish_phoneme_gesture(ctx);
        self.history_navigation = true;
        if let Some(previous) = self.undo.pop() {
            let next = self.restore_history(previous);
            self.redo.push(next);
        }
    }

    pub(crate) fn redo(&mut self, ctx: &eframe::egui::Context) {
        self.finish_phoneme_gesture(ctx);
        self.history_navigation = true;
        if let Some(next) = self.redo.pop() {
            let previous = self.restore_history(next);
            self.undo.push(previous);
        }
    }

    fn restore_history(&mut self, mut entry: Entry) -> Entry {
        entry.project.export_settings = self.project.export_settings.clone();
        self.query_basis_current &= crate::app::query_basis::matches(&entry.project, &self.project);
        self.portrait_state.clear_choices();
        self.invalidate_audio();
        let previous = Entry {
            project: std::mem::replace(&mut self.project, entry.project),
            selection: std::mem::replace(&mut self.selection, entry.selection),
        };
        self.selection.prune(&self.project);
        self.dirty = true;
        self.prepare_project();
        previous
    }
}
