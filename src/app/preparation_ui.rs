//! Apply query-derived timing before playback.
use crate::app::{editor::EditorApp, render};
use eframe::egui;

impl EditorApp {
    pub(crate) fn prepare_project(&mut self) {
        let Some(config) = self.render_config.clone() else {
            return;
        };
        if !self
            .project
            .blocks
            .iter()
            .any(|block| !block.text.trim().is_empty())
        {
            return;
        }
        // Editing may temporarily produce invalid dictionary/readings. Keep such
        // errors out of modal dialogs; explicit Play still reports the failure.
        let user_dictionary = match self.dictionary_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.render_status = error;
                return;
            }
        };
        if let Err(error) = self.render_worker.prepare(render::Request {
            config,
            project: self.project.clone(),
            user_dictionary,
        }) {
            self.render_status = error;
        }
    }

    pub(crate) fn poll_preparation(&mut self, ctx: &egui::Context) {
        if self.render_worker.preparing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(40));
        }
        let Some(result) = self.render_worker.poll_prepare() else {
            return;
        };
        let result = match result {
            Ok(result)
                if result.source.same_content(&self.project)
                    && self.dictionary_state.matches(&result.user_dictionary) =>
            {
                result
            }
            Ok(_) => return,
            Err(error) => {
                self.render_status = error;
                return;
            }
        };
        let applied = self
            .dictionary_snapshot()
            .and_then(|dictionary| apply(&self.project, &dictionary, &result));
        match applied {
            Ok(updated) => {
                // Rebind only after the authoritative application accepted this snapshot.
                let mut result = result;
                result.source = updated.clone();
                if let Err(error) = self.export.remember_preparation(result) {
                    self.render_status = error;
                }
                // Derived metadata is applied before the frame's history snapshot;
                // it cannot cancel synthesis or create a synthetic user edit.
                self.dirty |= self.project != updated;
                self.project = updated;
                self.query_basis_current = true;
            }
            Err(error) => self.render_status = error,
        }
    }
}

/// Shared by the editor and headless preparation diagnostics.
pub(crate) fn apply(
    source: &crate::app::document::Project,
    dictionary: &crate::app::dictionary::Snapshot,
    result: &render::PreparedProject,
) -> Result<crate::app::document::Project, String> {
    if !source.same_content(&result.source) || !dictionary.same(&result.user_dictionary) {
        return Err("Prepared source snapshot no longer matches".into());
    }
    let mut updated = source.clone();
    let indices: std::collections::BTreeMap<_, _> = updated
        .blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id, index))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for prepared in &result.blocks {
        let end = prepared.start + prepared.duration;
        if !prepared.start.is_finite()
            || !end.is_finite()
            || prepared.start < 0.0
            || prepared.duration < 0.0
            || end > 600.0
            || !seen.insert(prepared.id)
        {
            return Err("Invalid prepared block range".into());
        }
        let index = *indices
            .get(&prepared.id)
            .ok_or("Unknown prepared block ID")?;
        let block = &mut updated.blocks[index];
        block.timestamp = prepared.start;
        block.segments = prepared.segments.clone();
        block.sentences = prepared.sentences.clone();
        block.sentence_serial = block
            .sentence_serial
            .max(block.sentences.iter().map(|s| s.id).max().unwrap_or(0));
    }
    if seen.len() != updated.blocks.len() {
        return Err("Incomplete prepared block metadata".into());
    }
    let sentences: Vec<_> = updated
        .blocks
        .iter()
        .map(|block| block.sentence_ranges())
        .collect();
    for unit in &result.units {
        let index = *indices
            .get(&unit.block)
            .ok_or("Unknown prepared sentence block")?;
        let block = &updated.blocks[index];
        if sentences[index].get(unit.sentence.index) != Some(&unit.sentence)
            || !unit.start.is_finite()
            || !unit.duration.is_finite()
            || unit.start < 0.0
            || unit.duration < 0.0
            || unit.sample_start != (unit.start * 48_000.0) as usize
            || unit.segment_range.start > unit.segment_range.end
            || unit.segment_range.end > block.segments.len()
            || block.segments[unit.segment_range.clone()]
                .iter()
                .any(|segment| segment.sentence != unit.sentence)
        {
            return Err("Invalid prepared sentence address".into());
        }
    }
    updated.validate()?;
    Ok(updated)
}
