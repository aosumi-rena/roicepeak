//! Atomic SSML script replacement with durable pending phoneme annotations.
use super::{MAX_BYTES, read_bytes, ssml, text};
use crate::app::{
    document::{Block, OffsetMode, Project, VoiceRef, ssml as annotations},
    editor::EditorApp,
};

pub(super) fn replacement(
    script: ssml::Script,
    narrator: &VoiceRef,
    export_settings: crate::app::export::Settings,
) -> Result<Project, String> {
    let mut project = Project {
        blocks: Vec::new(),
        export_settings,
        ..Project::default()
    };
    for (index, speech) in script.blocks.into_iter().enumerate() {
        let mut block = Block::new(index as u64 + 1);
        // Voice name/lang attributes are not selectors on the native import route.
        block.narrator = narrator.clone();
        block.offset_mode = OffsetMode::PreviousEnd;
        block.offset_seconds = speech.delay_seconds;
        block.set_text(speech.text)?;
        let chunks = speech
            .chunks
            .into_iter()
            .map(|chunk| {
                let (morae, groups) = annotations::from_reading(chunk.reading);
                annotations::Unassigned {
                    surface: chunk.surface,
                    ph: chunk.ph,
                    morae,
                    groups,
                }
            })
            .collect();
        annotations::assign(&mut block, chunks)?;
        project.blocks.push(block);
    }
    project.validate()?;
    if serde_json::to_vec(&project)
        .map_err(|e| e.to_string())?
        .len()
        > MAX_BYTES
    {
        return Err("Imported project would exceed 16 MiB".into());
    }
    Ok(project)
}

impl EditorApp {
    pub(crate) fn import_ssml(&mut self) -> Result<(), String> {
        let Some(path) = rfd::FileDialog::new()
            .set_title(self.tr(
                "開くファイルを選択してください...",
                "Please select the file to open...",
            ))
            .add_filter("SSML", &["ssml"])
            .pick_file()
        else {
            return Ok(());
        };
        // Parse/decode and validate the entire proposed project before mutations.
        let input = text::decode(&read_bytes(&path)?)?;
        let script = ssml::parse(&input)?;
        let project = replacement(
            script,
            &self.default_narrator(),
            self.resources.export_settings.clone().unwrap_or_default(),
        )?;
        self.query_basis_current = false;
        self.invalidate_audio();
        self.native_save = None;
        self.project = project;
        self.portrait_state.clear_choices();
        self.filename = None;
        self.selection.clear();
        self.dirty = true;
        self.undo.clear();
        self.redo.clear();
        self.history_navigation = true;
        self.scroll = 0.0;
        self.prepare_project();
        Ok(())
    }
}
