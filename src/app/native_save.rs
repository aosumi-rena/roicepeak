use crate::app::{
    dictionary::Snapshot,
    document::Project,
    editor::{EditorApp, PendingAction},
    project_files,
    render::Config,
    save_revision,
};
use eframe::egui;
use std::path::PathBuf;

pub(crate) struct Pending {
    path: PathBuf,
    pub(crate) after_save: Option<PendingAction>,
    basis: Option<Basis>,
    generation: Option<u64>,
}

struct Basis {
    source: Project,
    config: Option<Config>,
    dictionary_path: Option<PathBuf>,
    dictionary: Snapshot,
}

impl EditorApp {
    pub(crate) fn request_save_path(&mut self, path: PathBuf) {
        let after_save = self.native_save.take().and_then(|save| save.after_save);
        self.native_save = Some(Pending {
            path,
            after_save,
            basis: None,
            generation: None,
        });
    }

    fn save_dictionary_path(&self) -> Option<PathBuf> {
        self.render_config
            .as_ref()
            .and_then(|config| config.dictionary.clone())
            .or_else(|| self.resources.dictionary.clone())
    }

    fn save_basis_current(&self, basis: &Basis) -> bool {
        save_revision::matches(&self.project, &basis.source)
            && self.render_config == basis.config
            && self.save_dictionary_path() == basis.dictionary_path
            && self.dictionary_state.matches(&basis.dictionary)
    }

    pub(crate) fn poll_native_save(&mut self, ctx: &egui::Context) {
        let Some(mut save) = self.native_save.take() else {
            return;
        };
        let required = match crate::app::native_interop::requires_preparation(&self.project) {
            Ok(required) => required,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        if !required {
            self.finish_native_save(save, &self.project.clone(), ctx);
            return;
        }
        if save
            .basis
            .as_ref()
            .is_none_or(|basis| !self.save_basis_current(basis))
        {
            let dictionary = match self.dictionary_snapshot() {
                Ok(dictionary) => dictionary,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            };
            save.basis = Some(Basis {
                source: self.project.clone(),
                config: self.render_config.clone(),
                dictionary_path: self.save_dictionary_path(),
                dictionary,
            });
            save.generation = None;
        }
        if let Some((generation, result)) = self.save_worker.poll() {
            if save.generation == Some(generation) {
                match result {
                    Ok(project) => self.finish_native_save(save, &project, ctx),
                    Err(error) => self.error = Some(error),
                }
                return;
            }
        }
        if save.generation.is_none() {
            let basis = save.basis.as_ref().unwrap();
            match self.save_worker.submit(
                basis.source.clone(),
                basis.dictionary_path.clone(),
                basis.dictionary.clone(),
            ) {
                Ok(generation) => save.generation = Some(generation),
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            }
        }
        self.native_save = Some(save);
        ctx.request_repaint_after(std::time::Duration::from_millis(40));
    }

    fn finish_native_save(&mut self, save: Pending, project: &Project, ctx: &egui::Context) {
        match project_files::write(&save.path, project, self.render_config.as_ref()) {
            Ok(()) => {
                if let Err(error) = self.recent_projects.record(&save.path) {
                    self.error = Some(error);
                }
                self.filename = Some(save.path);
                self.dirty = false;
                if let Some(action) = save.after_save {
                    self.execute_action(action, ctx);
                }
            }
            Err(error) => self.error = Some(error),
        }
    }
}
