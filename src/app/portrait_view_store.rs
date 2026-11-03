//! Portrait view preferences belonging only to this editor.
use super::math::View;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::PathBuf,
};

const LIMIT: u64 = 2 * 1024 * 1024;
const MAX_IDENTITIES: usize = 256;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    format: u32,
    views: BTreeMap<String, View>,
}

#[derive(Default)]
pub(super) struct Store {
    pub views: BTreeMap<String, View>,
    loaded: bool,
    writable: bool,
    dirty: bool,
    pub problem: Option<String>,
}

fn path() -> Result<PathBuf, String> {
    let root =
        crate::platform::config_dir().ok_or("Local application data directory is unavailable")?;
    if !root.is_absolute() {
        return Err("Local application data directory is not absolute".into());
    }
    Ok(root.join("portrait-views.json"))
}

impl Store {
    pub fn load_once(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        match Self::read() {
            Ok(views) => {
                self.views = views;
                self.writable = true;
            }
            Err(error) => self.problem = Some(error),
        }
    }

    fn read() -> Result<BTreeMap<String, View>, String> {
        let file = match fs::File::open(path()?) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => return Err(format!("Cannot read portrait views: {error}")),
        };
        let mut bytes = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > LIMIT {
            return Err("Portrait view file exceeds 2 MiB".into());
        }
        let file: File = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid portrait view file: {e}"))?;
        if file.format != 1
            || file.views.len() > MAX_IDENTITIES
            || file.views.iter().any(|(identity, view)| {
                identity.is_empty() || identity.len() > 4096 || !view.valid()
            })
        {
            return Err("Unsupported or invalid portrait view preferences".into());
        }
        Ok(file.views)
    }

    pub fn block_writes(&mut self, error: String) {
        self.writable = false;
        self.problem = Some(error);
    }

    pub fn record(&mut self, identity: &str, view: View, persist: bool) {
        if self.views.get(identity) != Some(&view) {
            if !self.views.contains_key(identity) && self.views.len() == MAX_IDENTITIES {
                self.views.pop_first();
            }
            self.views.insert(identity.to_owned(), view);
            self.dirty = true;
        }
        if persist && self.dirty && self.writable {
            match self.save() {
                Ok(()) => self.dirty = false,
                Err(error) => self.problem = Some(error),
            }
        }
    }

    fn save(&self) -> Result<(), String> {
        let data = File {
            format: 1,
            views: self.views.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > LIMIT {
            return Err("Portrait view file exceeds 2 MiB".into());
        }
        let destination = path()?;
        fs::create_dir_all(
            destination
                .parent()
                .ok_or("Invalid portrait view directory")?,
        )
        .map_err(|e| e.to_string())?;
        let temporary = destination.with_extension(format!("json.{}.tmp", std::process::id()));
        let mut created = false;
        let result = (|| -> Result<(), String> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| format!("Cannot create portrait view transaction: {e}"))?;
            created = true;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            fs::rename(&temporary, &destination)
                .map_err(|e| format!("Cannot commit portrait views: {e}"))
        })();
        if result.is_err() && created {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}
