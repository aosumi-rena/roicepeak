//! One serial worker for lexical native-save preparation.
use crate::app::{dictionary::Snapshot, document::Project};
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
};

struct Job {
    generation: u64,
    project: Project,
    dictionary_path: PathBuf,
    dictionary: Snapshot,
}

#[derive(Default)]
struct State {
    generation: u64,
    pending: Option<Job>,
    completed: Option<(u64, Result<Project, String>)>,
    closed: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

pub(crate) struct Worker {
    shared: Arc<Shared>,
    start_error: Option<String>,
}

impl Default for Worker {
    fn default() -> Self {
        let shared = Arc::new(Shared::default());
        let worker_shared = Arc::clone(&shared);
        let start_error = std::thread::Builder::new()
            .name("roicepeak-save-lexical".into())
            .spawn(move || run(worker_shared))
            .err()
            .map(|error| format!("Cannot start lexical save worker: {error}"));
        Self {
            shared,
            start_error,
        }
    }
}

impl Worker {
    pub(crate) fn submit(
        &mut self,
        project: Project,
        dictionary_path: PathBuf,
        dictionary: Snapshot,
    ) -> Result<u64, String> {
        if let Some(error) = &self.start_error {
            return Err(error.clone());
        }
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| "Lexical save worker failed")?;
        if state.closed {
            return Err("Lexical save worker is closed".into());
        }
        let generation = state
            .generation
            .checked_add(1)
            .ok_or("Lexical save generation exhausted")?;
        state.generation = generation;
        state.completed = None;
        state.pending = Some(Job {
            generation,
            project,
            dictionary_path,
            dictionary,
        });
        self.shared.wake.notify_one();
        Ok(generation)
    }

    pub(crate) fn poll(&mut self) -> Option<(u64, Result<Project, String>)> {
        let mut state = self.shared.state.lock().ok()?;
        let completed = state.completed.take()?;
        (completed.0 == state.generation && !state.closed).then_some(completed)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.closed = true;
            state.pending = None;
            state.completed = None;
        }
        self.shared.wake.notify_one();
    }
}

fn run(shared: Arc<Shared>) {
    loop {
        let job = {
            let Ok(mut state) = shared.state.lock() else {
                return;
            };
            while state.pending.is_none() && !state.closed {
                state = match shared.wake.wait(state) {
                    Ok(state) => state,
                    Err(_) => return,
                };
            }
            if state.closed {
                return;
            }
            state.pending.take().unwrap()
        };
        let Job {
            generation,
            project,
            dictionary_path,
            dictionary,
        } = job;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::app::save_lexical::prepare(&project, &dictionary_path, &dictionary)
        }))
        .unwrap_or_else(|_| Err("Lexical save preparation failed".into()));
        drop(project);
        let Ok(mut state) = shared.state.lock() else {
            return;
        };
        if state.closed {
            return;
        }
        if generation == state.generation {
            state.completed = Some((generation, result));
        }
    }
}
