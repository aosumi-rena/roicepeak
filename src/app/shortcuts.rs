//! Editor command bindings and focus-aware dispatch.
use serde::{Deserialize, Serialize};
#[path = "shortcut_key.rs"]
mod key;
#[path = "shortcut_slot.rs"]
mod slot;
pub use key::{Chord, Key, Modifiers};
pub use slot::Slot;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Command {
    NewProject,
    OpenProject,
    ImportSrt,
    ImportText,
    ImportCsv,
    ImportSsml,
    SaveProject,
    SaveProjectAs,
    Export,
    ExportBlock,
    ReplaceVoices,
    Preferences,
    KeyboardShortcuts,
    CheckForUpdates,
    About,
    Help,
    Undo,
    Redo,
    FindReplace,
    Dictionary,
    RegisterWord,
    ShowSpeed,
    ShowPitch,
    ShowPause,
    ShowEmotion,
    PlayPause,
    PlayBlock,
    ToggleLoop,
    SkipStart,
    SkipEnd,
    PreviousBlock,
    NextBlock,
    Copy,
    Paste,
    Cut,
    SelectAll,
    DeleteSelection,
}

impl Command {
    pub fn label(self) -> &'static str {
        match self {
            Self::NewProject => "New project",
            Self::OpenProject => "Open project",
            Self::ImportSrt => "Import SRT",
            Self::ImportText => "Import text",
            Self::ImportCsv => "Import CSV",
            Self::ImportSsml => "Import SSML",
            Self::SaveProject => "Save project",
            Self::SaveProjectAs => "Save project as",
            Self::Export => "Export",
            Self::ExportBlock => "Export block",
            Self::ReplaceVoices => "Replace voices",
            Self::Preferences => "Preferences",
            Self::KeyboardShortcuts => "Keyboard shortcuts",
            Self::CheckForUpdates => "Check for updates",
            Self::About => "About",
            Self::Help => "Help",
            Self::Undo => "Undo",
            Self::Redo => "Redo",
            Self::FindReplace => "Find and replace",
            Self::Dictionary => "User dictionary",
            Self::RegisterWord => "Register word",
            Self::ShowSpeed => "Show speed panel",
            Self::ShowPitch => "Show pitch panel",
            Self::ShowPause => "Show pause panel",
            Self::ShowEmotion => "Show emotion panel",
            Self::PlayPause => "Play or pause",
            Self::PlayBlock => "Play current block",
            Self::ToggleLoop => "Toggle loop playback",
            Self::SkipStart => "Skip to start",
            Self::SkipEnd => "Skip to end",
            Self::PreviousBlock => "Previous block",
            Self::NextBlock => "Next block",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Cut => "Cut",
            Self::SelectAll => "Select all",
            Self::DeleteSelection => "Delete selection",
        }
    }

    /// Reference menu command identifier.
    pub fn native_id(self) -> Option<u32> {
        Some(match self {
            Self::NewProject => 0x102,
            Self::OpenProject => 0x103,
            Self::SaveProject => 0x104,
            Self::SaveProjectAs => 0x105,
            Self::ImportSrt => 0x106,
            Self::ImportText => 0x107,
            Self::ImportCsv => 0x108,
            Self::ImportSsml => 0x109,
            Self::Export => 0x10a,
            Self::ExportBlock => 0x10b,
            Self::CheckForUpdates => 0x10c,
            Self::Preferences => 0x10d,
            Self::KeyboardShortcuts => 0x10e,
            Self::About => 0x10f,
            Self::ReplaceVoices => 0x112,
            Self::RegisterWord => 0x202,
            Self::Dictionary => 0x203,
            Self::Undo => 0x1102,
            Self::Redo => 0x1103,
            Self::FindReplace => 0x1104,
            Self::DeleteSelection => 0x1105,
            Self::PlayPause => 0x1106,
            Self::PlayBlock => 0x1107,
            Self::ToggleLoop => 0x1108,
            Self::SkipStart => 0x1109,
            Self::SkipEnd => 0x110a,
            Self::NextBlock => 0x110b,
            Self::PreviousBlock => 0x110c,
            Self::ShowSpeed => 0x110d,
            Self::ShowPitch => 0x110e,
            Self::ShowPause => 0x110f,
            Self::ShowEmotion => 0x1110,
            Self::Help | Self::Copy | Self::Paste | Self::Cut | Self::SelectAll => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingError {
    InvalidKey,
    IncompleteSlots,
    UnsupportedFormat,
    Conflict(Slot),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShortcutFile {
    format: u32,
    slots: std::collections::BTreeMap<Slot, Option<Chord>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutMap {
    slots: std::collections::BTreeMap<Slot, Option<Chord>>,
}
impl ShortcutMap {
    pub fn defaults() -> Self {
        Self {
            slots: Slot::all()
                .map(|slot| (slot, slot.default_chord()))
                .collect(),
        }
    }
    pub fn chord(&self, slot: Slot) -> Option<Chord> {
        self.slots[&slot]
    }
    pub fn primary_chord(&self, command: Command) -> Option<Chord> {
        Slot::all()
            .find(|slot| slot.command() == command)
            .and_then(|slot| self.chord(slot))
    }
    pub fn resolve(&self, chord: Chord) -> Option<Command> {
        Slot::all()
            .find(|&slot| self.chord(slot) == Some(chord))
            .map(Slot::command)
    }
    pub fn conflicts(&self, slot: Slot, chord: Option<Chord>) -> Vec<Slot> {
        let Some(chord) = chord else {
            return Vec::new();
        };
        if matches!(chord.key, Key::Delete | Key::Backspace) {
            return Vec::new();
        }
        Slot::all()
            .filter(|&other| other != slot && self.chord(other) == Some(chord))
            .collect()
    }
    pub fn assign(&mut self, slot: Slot, chord: Option<Chord>) -> Result<(), BindingError> {
        let chord = chord.filter(|chord| !matches!(chord.key, Key::Delete | Key::Backspace));
        if chord.is_some_and(|chord| !chord.key.valid()) {
            return Err(BindingError::InvalidKey);
        }
        if let Some(other) = self.conflicts(slot, chord).first() {
            return Err(BindingError::Conflict(*other));
        }
        self.slots.insert(slot, chord);
        Ok(())
    }
    pub fn reset(&mut self) {
        *self = Self::defaults();
    }
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec_pretty(&ShortcutFile {
            format: 2,
            slots: self.slots.clone(),
        })
        .map_err(|e| e.to_string())
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let file: ShortcutFile = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if file.format != 2 {
            return Err(format!(
                "Invalid shortcuts: {:?}",
                BindingError::UnsupportedFormat
            ));
        }
        if file.slots.len() != Slot::all().count()
            || Slot::all().any(|slot| !file.slots.contains_key(&slot))
        {
            return Err(format!(
                "Invalid shortcuts: {:?}",
                BindingError::IncompleteSlots
            ));
        }
        let mut result = Self {
            slots: Slot::all().map(|slot| (slot, None)).collect(),
        };
        for slot in Slot::all() {
            result
                .assign(slot, file.slots[&slot])
                .map_err(|e| format!("Invalid shortcuts: {e:?}"))?;
        }
        Ok(result)
    }
}

fn local_path() -> Result<std::path::PathBuf, String> {
    let root =
        crate::platform::config_dir().ok_or("Local application data directory is unavailable")?;
    if !root.is_absolute() {
        return Err("Local application data directory is not absolute".into());
    }
    Ok(root.join("shortcuts.json"))
}
impl ShortcutMap {
    pub fn load_local() -> Result<Self, String> {
        use std::io::Read;
        let file = match std::fs::File::open(local_path()?) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::defaults());
            }
            Err(error) => return Err(format!("Cannot read shortcut settings: {error}")),
        };
        let mut bytes = Vec::new();
        file.take(65_537)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 65_536 {
            return Err("Shortcut file exceeds 64 KiB".into());
        }
        Self::decode(&bytes)
    }
    pub fn save_local(&self) -> Result<(), String> {
        use std::io::Write;
        let destination = local_path()?;
        let parent = destination
            .parent()
            .ok_or("Shortcut settings parent is unavailable")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = parent.join(format!("shortcuts-{}.tmp", std::process::id()));
        let mut created = false;
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| format!("Cannot stage shortcut settings: {e}"))?;
            created = true;
            file.write_all(&self.encode()?).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            std::fs::rename(&temporary, &destination)
                .map_err(|e| format!("Cannot commit shortcut settings: {e}"))
        })();
        if result.is_err() && created {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }
}
