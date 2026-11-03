use super::{Entry, Snapshot, Store};
use crate::app::editor::EditorApp;

pub(crate) struct State {
    store: Option<Store>,
    error: Option<String>,
    opened: bool,
    changed: bool,
}

impl State {
    pub(crate) fn load() -> Self {
        match Store::load() {
            Ok(store) => Self {
                store: Some(store),
                error: None,
                opened: false,
                changed: false,
            },
            Err(error) => Self {
                store: None,
                error: Some(error),
                opened: false,
                changed: false,
            },
        }
    }

    pub(crate) fn display_snapshot(&self) -> Snapshot {
        self.store
            .as_ref()
            .map_or_else(Snapshot::empty, Store::snapshot)
    }

    pub(crate) fn snapshot(&self) -> Result<Snapshot, String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        self.store
            .as_ref()
            .map(Store::snapshot)
            .ok_or_else(|| "User dictionary is unavailable".into())
    }

    pub(crate) fn matches(&self, snapshot: &Snapshot) -> bool {
        self.snapshot().is_ok_and(|current| current.same(snapshot))
    }

    fn begin(&mut self) -> Result<bool, String> {
        if self.opened {
            return Ok(false);
        }
        self.opened = true;
        let recovering = self.error.is_some();
        let result = if let Some(store) = self.store.as_mut() {
            store.reload()
        } else {
            Store::load().map(|store| {
                self.store = Some(store);
                true
            })
        };
        match result {
            Ok(changed) => {
                let refresh = changed || recovering;
                self.error = None;
                self.changed |= refresh;
                Ok(refresh)
            }
            Err(error) => {
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }

    fn commit(&mut self, expected: &Snapshot, entries: Vec<Entry>) -> Result<bool, String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let changed = self
            .store
            .as_mut()
            .ok_or("User dictionary is unavailable")?
            .commit(expected, entries)?;
        self.changed |= changed;
        Ok(changed)
    }
}

impl EditorApp {
    pub(crate) fn dictionary_snapshot(&self) -> Result<Snapshot, String> {
        self.dictionary_state.snapshot()
    }

    pub(crate) fn dictionary_writable(&self) -> bool {
        self.dictionary_state.snapshot().is_ok()
    }

    fn dictionary_changed(&mut self) {
        self.query_basis_current = false;
        self.invalidate_audio();
        self.export.forget_preparation();
    }

    pub(crate) fn commit_dictionary(
        &mut self,
        expected: &Snapshot,
        entries: Vec<Entry>,
    ) -> Result<(), String> {
        if self.dictionary_state.commit(expected, entries)? {
            self.dictionary_changed();
        }
        Ok(())
    }

    pub(crate) fn begin_dictionary_window(&mut self) {
        match self.dictionary_state.begin() {
            Ok(true) => {
                self.dictionary_ui.selected = None;
                self.dictionary_changed();
            }
            Ok(false) => (),
            Err(error) => {
                self.dictionary_changed();
                self.dictionary_ui.file_error = Some(error);
            }
        }
    }

    pub(crate) fn end_dictionary_window(&mut self) {
        self.dictionary_state.opened = false;
        let changed = std::mem::take(&mut self.dictionary_state.changed);
        self.dictionary_ui.close_dialog();
        if changed {
            self.prepare_project();
        }
    }
}
