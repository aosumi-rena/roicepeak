use crate::app::{
    dictionary::Snapshot,
    document::Project,
    editor::{EditorApp, PendingAction},
    preparation_ui, project_files,
    render::{self, Config},
};
use eframe::egui;
use std::path::PathBuf;

pub(crate) struct Pending {
    path: PathBuf,
    pub(crate) after_save: Option<PendingAction>,
    basis: Option<Basis>,
    submitted: bool,
    ready: bool,
}

struct Basis {
    source: Project,
    config: Config,
    dictionary: Snapshot,
}

impl EditorApp {
    pub(crate) fn request_save_path(&mut self, path: PathBuf) {
        let after_save = self.native_save.take().and_then(|save| save.after_save);
        self.native_save = Some(Pending {
            path,
            after_save,
            basis: None,
            submitted: false,
            ready: false,
        });
    }

    pub(crate) fn save_preparation_pending(&self) -> bool {
        self.native_save.as_ref().is_some_and(|save| save.submitted)
    }

    fn save_basis_current(&self, basis: &Basis) -> bool {
        self.project == basis.source
            && self.render_config.as_ref() == Some(&basis.config)
            && self.dictionary_state.matches(&basis.dictionary)
    }

    pub(crate) fn poll_native_save(&mut self, ctx: &egui::Context) {
        let Some(mut save) = self.native_save.take() else {
            return;
        };
        let needs_preparation = project_files::native(&save.path)
            && self
                .project
                .blocks
                .iter()
                .any(|block| !block.text.trim().is_empty())
            && if save
                .basis
                .as_ref()
                .is_some_and(|basis| self.save_basis_current(basis))
            {
                true
            } else {
                match crate::app::native_interop::requires_preparation(&self.project) {
                    Ok(required) => required,
                    Err(error) => {
                        self.error = Some(error);
                        return;
                    }
                }
            };
        if !needs_preparation
            || (save.basis.is_none() && self.query_basis_current)
            || (save.ready
                && save
                    .basis
                    .as_ref()
                    .is_some_and(|basis| self.save_basis_current(basis)))
        {
            self.finish_native_save(save, ctx);
            return;
        }
        if save
            .basis
            .as_ref()
            .is_none_or(|basis| !self.save_basis_current(basis))
        {
            let Some(config) = self.render_config.clone() else {
                self.error = Some("Native Save requires available narrator resources".into());
                return;
            };
            let dictionary = match self.dictionary_snapshot() {
                Ok(dictionary) => dictionary,
                Err(error) => {
                    self.error = Some(error);
                    return;
                }
            };
            save.basis = Some(Basis {
                source: self.project.clone(),
                config,
                dictionary,
            });
            save.submitted = false;
            save.ready = false;
        }
        if save.submitted && !self.render_worker.preparing() {
            if let Some(result) = self.render_worker.poll_prepare() {
                self.native_save = Some(save);
                self.accept_save_preparation(result, ctx);
                return;
            }
        }
        if !save.submitted || !self.render_worker.preparing() {
            let basis = save.basis.as_ref().unwrap();
            if let Err(error) = self.render_worker.prepare(render::Request {
                config: basis.config.clone(),
                project: basis.source.clone(),
                user_dictionary: basis.dictionary.clone(),
            }) {
                self.error = Some(error);
                return;
            }
            save.submitted = true;
        }
        self.native_save = Some(save);
        ctx.request_repaint_after(std::time::Duration::from_millis(40));
    }

    pub(crate) fn accept_save_preparation(
        &mut self,
        result: Result<render::PreparedProject, String>,
        ctx: &egui::Context,
    ) {
        let Some(mut save) = self.native_save.take() else {
            return;
        };
        let Some(basis) = save.basis.as_ref() else {
            self.native_save = Some(save);
            return;
        };
        if !self.save_basis_current(basis) {
            save.submitted = false;
            self.native_save = Some(save);
            return;
        }
        let mut result = match result {
            Ok(result)
                if result.source == basis.source
                    && result.config == basis.config
                    && result.user_dictionary.same(&basis.dictionary) =>
            {
                result
            }
            Ok(_) => {
                save.submitted = false;
                self.native_save = Some(save);
                return;
            }
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let config = basis.config.clone();
        let dictionary = basis.dictionary.clone();
        let updated = match preparation_ui::apply(&basis.source, &basis.dictionary, &result) {
            Ok(updated) => updated,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        self.dirty |= self.project != updated;
        self.project = updated;
        self.query_basis_current = true;
        result.source = self.project.clone();
        if let Err(error) = self.export.remember_preparation(result) {
            self.render_status = error;
        }
        save.basis = Some(Basis {
            source: self.project.clone(),
            config,
            dictionary,
        });
        save.submitted = false;
        save.ready = true;
        self.native_save = Some(save);
        ctx.request_repaint();
    }

    fn finish_native_save(&mut self, save: Pending, ctx: &egui::Context) {
        match project_files::write(&save.path, &self.project, self.render_config.as_ref()) {
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
