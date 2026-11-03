//! Application-owned dictionary storage, independent of project history.
use super::{Entry, file};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Snapshot {
    entries: Arc<Vec<Entry>>,
}

impl Snapshot {
    pub(crate) fn empty() -> Self {
        Self {
            entries: Arc::new(Vec::new()),
        }
    }

    pub(crate) fn entries(&self) -> &[Entry] {
        self.entries.as_slice()
    }

    pub(crate) fn read(path: &Path) -> Result<Self, String> {
        Ok(Self {
            entries: Arc::new(file::read(path)?),
        })
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.entries.len() > 10_000 {
            return Err("Dictionary entry count exceeds 10000".into());
        }
        for entry in self.entries.iter() {
            entry.validate_stored()?;
        }
        Ok(())
    }

    pub(crate) fn shared(&self) -> Arc<Vec<Entry>> {
        Arc::clone(&self.entries)
    }

    pub(crate) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.entries, &other.entries) || self.entries == other.entries
    }
}

pub(crate) struct Store {
    path: PathBuf,
    snapshot: Snapshot,
}

impl Store {
    pub(crate) fn load() -> Result<Self, String> {
        let root = crate::platform::config_dir()
            .filter(|root| root.is_absolute())
            .ok_or("ROICEPEAK user dictionary directory is unavailable")?;
        Self::open(root.join("user-dictionary.vdc2"))
    }

    pub(crate) fn open(path: PathBuf) -> Result<Self, String> {
        if !path.is_absolute() {
            return Err("User dictionary storage path must be absolute".into());
        }
        let entries = read(&path)?;
        Ok(Self {
            path,
            snapshot: Snapshot {
                entries: Arc::new(entries),
            },
        })
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        self.snapshot.clone()
    }

    pub(crate) fn entries(&self) -> &[Entry] {
        self.snapshot.entries()
    }

    /// Reload on opening the dictionary window; failed reads preserve active data.
    pub(crate) fn reload(&mut self) -> Result<bool, String> {
        let entries = read(&self.path)?;
        if self.entries() == entries.as_slice() {
            return Ok(false);
        }
        self.snapshot = Snapshot {
            entries: Arc::new(entries),
        };
        Ok(true)
    }

    /// Encode and persist a complete staged mutation before publishing it.
    pub(crate) fn commit(
        &mut self,
        expected: &Snapshot,
        entries: Vec<Entry>,
    ) -> Result<bool, String> {
        if !self.snapshot.same(expected) {
            return Err("User dictionary changed before this edit was committed".into());
        }
        if self.entries() == entries.as_slice() {
            return Ok(false);
        }
        let bytes = file::encode(&entries)?;
        let parent = self
            .path
            .parent()
            .ok_or("User dictionary parent is unavailable")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = self
            .path
            .with_extension(format!("vdc2.{}.tmp", std::process::id()));
        let mut created = false;
        let result = (|| -> Result<(), String> {
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| error.to_string())?;
            created = true;
            output
                .write_all(&bytes)
                .map_err(|error| error.to_string())?;
            output.sync_all().map_err(|error| error.to_string())?;
            drop(output);
            fs::rename(&temporary, &self.path).map_err(|error| error.to_string())
        })();
        if result.is_err() && created {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        self.snapshot = Snapshot {
            entries: Arc::new(entries),
        };
        Ok(true)
    }
}

fn read(path: &Path) -> Result<Vec<Entry>, String> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => file::read(path),
        Ok(_) => Err("User dictionary storage is not a regular file".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    }
}
