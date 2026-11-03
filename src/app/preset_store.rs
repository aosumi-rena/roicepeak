//! Ordered user presets belonging exclusively to this editor.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::PathBuf,
};

const LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Values {
    pub voice: Option<[f32; 4]>,
    pub emotions: BTreeMap<String, f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Preset {
    pub name: String,
    pub values: Values,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Store {
    format: u32,
    pub narrators: BTreeMap<String, Vec<Preset>>,
    pub corrections: BTreeMap<String, Vec<Preset>>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            format: 2,
            narrators: BTreeMap::new(),
            corrections: BTreeMap::new(),
        }
    }
}

fn path() -> Result<PathBuf, String> {
    let root =
        crate::platform::config_dir().ok_or("Local application data directory is unavailable")?;
    if !root.is_absolute() {
        return Err("Local application data directory is not absolute".into());
    }
    Ok(root.join("emotion-presets.json"))
}

impl Store {
    pub fn load() -> Result<Self, String> {
        let file = match fs::File::open(path()?) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(format!("Cannot read emotion presets: {error}")),
        };
        let mut bytes = Vec::new();
        file.take(LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > LIMIT {
            return Err("Emotion preset file exceeds 4 MiB".into());
        }
        let store: Self = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid emotion preset file: {e}"))?;
        store.validate()?;
        Ok(store)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != 2 || self.narrators.len() > 256 || self.corrections.len() > 256 {
            return Err("Unsupported emotion preset store format or narrator count".into());
        }
        let mut total = 0;
        for (identity, presets) in self.narrators.iter().chain(&self.corrections) {
            if identity.is_empty() || identity.len() > 4096 || presets.len() > 1024 {
                return Err("Emotion preset narrator or count limit".into());
            }
            let mut names = std::collections::BTreeSet::new();
            for preset in presets {
                total += 1;
                if preset.name.is_empty()
                    || preset.name.len() > 4096
                    || !names.insert(&preset.name)
                    || preset.values.emotions.len() > 1024
                    || preset.values.emotions.iter().any(|(key, value)| {
                        key.is_empty()
                            || key.len() > 4096
                            || !value.is_finite()
                            || !(value * 100.0).is_finite()
                    })
                    || preset.values.voice.is_some_and(|values| {
                        values[3] < 0.0
                            || values
                                .iter()
                                .any(|v| !v.is_finite() || !(v * 100.0).is_finite())
                    })
                {
                    return Err("Invalid emotion preset name or parameter".into());
                }
            }
        }
        if total > 4096 {
            return Err("Emotion preset total count limit".into());
        }
        Ok(())
    }

    pub fn save(&self) -> Result<(), String> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if bytes.len() > LIMIT {
            return Err("Emotion preset file exceeds 4 MiB".into());
        }
        let destination = path()?;
        fs::create_dir_all(destination.parent().ok_or("Invalid preset directory")?)
            .map_err(|e| e.to_string())?;
        let temporary = destination.with_extension(format!("json.{}.tmp", std::process::id()));
        let mut created = false;
        let result = (|| -> Result<(), String> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| format!("Cannot create preset transaction: {e}"))?;
            created = true;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            fs::rename(&temporary, &destination)
                .map_err(|e| format!("Cannot commit emotion presets: {e}"))
        })();
        // Only this invocation's failed temporary file may be removed.
        if result.is_err() && created {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}
