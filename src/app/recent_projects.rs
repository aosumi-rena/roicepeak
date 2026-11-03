use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(crate) struct RecentProjects {
    paths: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    paths: Vec<PathBuf>,
}

fn storage_path() -> Result<PathBuf, String> {
    crate::platform::config_dir()
        .filter(|path| path.is_absolute())
        .map(|path| path.join("recent-projects.json"))
        .ok_or_else(|| "ROICEPEAK recent projects directory is unavailable".into())
}

impl RecentProjects {
    pub(crate) fn load() -> Result<Self, String> {
        let path = storage_path()?;
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.to_string()),
        };
        let mut bytes = Vec::new();
        file.take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() > 65_536 {
            return Err("Recent projects file exceeds 64 KiB".into());
        }
        let saved: Saved = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if saved.version != 1 || saved.paths.len() > 4 {
            return Err("Invalid recent projects file".into());
        }
        for (index, path) in saved.paths.iter().enumerate() {
            if !path.is_absolute()
                || path.as_os_str().is_empty()
                || saved.paths[..index]
                    .iter()
                    .any(|old| old.as_os_str() == path.as_os_str())
            {
                return Err("Invalid recent project path".into());
            }
        }
        Ok(Self { paths: saved.paths })
    }

    pub(crate) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub(crate) fn record(&mut self, path: &Path) -> Result<(), String> {
        if !path.is_absolute() {
            return Err("Recent project path must be absolute".into());
        }
        if self
            .paths
            .last()
            .is_some_and(|last| last.as_os_str() == path.as_os_str())
        {
            return Ok(());
        }
        let mut paths = self.paths.clone();
        paths.retain(|old| old.as_os_str() != path.as_os_str());
        paths.push(path.to_owned());
        if paths.len() > 4 {
            paths.drain(..paths.len() - 4);
        }
        let bytes = serde_json::to_vec_pretty(&Saved {
            version: 1,
            paths: paths.clone(),
        })
        .map_err(|error| error.to_string())?;
        if bytes.len() > 65_536 {
            return Err("Recent projects file exceeds 64 KiB".into());
        }
        let destination = storage_path()?;
        let parent = destination
            .parent()
            .ok_or("Recent projects parent is unavailable")?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let temporary = destination.with_extension(format!("json.{}.tmp", std::process::id()));
        let mut created = false;
        let result = (|| -> Result<(), String> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|error| error.to_string())?;
            created = true;
            file.write_all(&bytes).map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            drop(file);
            fs::rename(&temporary, &destination).map_err(|error| error.to_string())
        })();
        if result.is_err() && created {
            let _ = fs::remove_file(&temporary);
        }
        result?;
        self.paths = paths;
        Ok(())
    }
}
