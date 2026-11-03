//! ROICEPEAK resource locations and narrator preferences.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locations {
    #[serde(default)]
    pub locale: crate::app::i18n::Locale,
    pub voice_roots: Vec<PathBuf>,
    pub dictionary: Option<PathBuf>,
    pub preferred_voice: Option<String>,
    #[serde(default = "portrait_visible")]
    pub show_portrait: bool,
    #[serde(default)]
    pub allow_out_of_range: bool,
    #[serde(default)]
    pub allow_negative: bool,
    #[serde(default)]
    pub export_settings: Option<crate::app::export::Settings>,
    #[serde(default)]
    pub expanded_corrections: BTreeSet<String>,
}

fn portrait_visible() -> bool {
    true
}

impl Default for Locations {
    fn default() -> Self {
        Self {
            locale: crate::app::i18n::Locale::default(),
            voice_roots: Vec::new(),
            dictionary: None,
            preferred_voice: None,
            show_portrait: true,
            allow_out_of_range: false,
            allow_negative: false,
            export_settings: None,
            expanded_corrections: BTreeSet::new(),
        }
    }
}

fn preferences_path() -> Option<PathBuf> {
    crate::platform::config_dir().map(|path| path.join("preferences.json"))
}

impl Locations {
    pub fn value_policy(&self) -> roicepeak::scalar_layers::ValuePolicy {
        roicepeak::scalar_layers::ValuePolicy {
            allow_out_of_range: self.allow_out_of_range,
            allow_negative: self.allow_negative,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        if self
            .expanded_corrections
            .iter()
            .any(|identity| identity.is_empty() || identity.len() > 4096)
        {
            return Err("Invalid narrator correction expansion identity".into());
        }
        if let Some(settings) = &self.export_settings {
            settings.validate_stored()?;
        }
        if self.voice_roots.len() > 32
            || self
                .voice_roots
                .iter()
                .any(|path| path.as_os_str().is_empty() || !path.is_absolute())
            || self
                .dictionary
                .as_ref()
                .is_some_and(|path| path.as_os_str().is_empty() || !path.is_absolute())
            || self
                .preferred_voice
                .as_ref()
                .is_some_and(|key| key.len() > 4096)
        {
            return Err(
                "Resource search paths must be absolute; at most 32 sources are supported".into(),
            );
        }
        Ok(())
    }
    pub fn load() -> Result<Self, String> {
        let user = preferences_path();
        let packaged = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("Application directory unavailable")?
            .join("resources/locations.json");
        let path = user
            .as_deref()
            .filter(|path| path.is_file())
            .unwrap_or(&packaged);
        if !path.is_file() {
            return Ok(Self::default());
        }
        Self::read(path)
    }

    /// Read an explicit resource configuration without changing saved preferences.
    pub fn read(path: &Path) -> Result<Self, String> {
        let path = fs::canonicalize(path).map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .map_err(|e| e.to_string())?
            .take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 65_536 {
            return Err("ROICEPEAK resource locations exceed 64 KiB".into());
        }
        let mut locations: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if locations.voice_roots.len() > 32
            || locations
                .preferred_voice
                .as_ref()
                .is_some_and(|key| key.len() > 4096)
        {
            return Err("Invalid resource locations".into());
        }
        let base = path.parent().unwrap_or(Path::new("."));
        for root in &mut locations.voice_roots {
            if root.is_relative() {
                *root = base.join(&*root);
            }
        }
        if let Some(dictionary) = &mut locations.dictionary {
            if dictionary.is_relative() {
                *dictionary = base.join(&*dictionary);
            }
        }
        locations.validate()?;
        Ok(locations)
    }

    pub fn save(&self) -> Result<(), String> {
        self.validate()?;
        let path = preferences_path().ok_or("ROICEPEAK preferences directory unavailable")?;
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if bytes.len() > 65_536 {
            return Err("ROICEPEAK resource locations exceed 64 KiB".into());
        }
        fs::write(path, bytes).map_err(|e| e.to_string())
    }
}
