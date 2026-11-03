use super::{Captions, MAX_BYTES, read_srt};
use crate::app::{
    document::{Block, OffsetMode, Project},
    editor::EditorApp,
};

fn replacement(
    captions: Captions,
    narrator: &crate::app::document::VoiceRef,
    export_settings: crate::app::export::Settings,
) -> Result<(Project, Option<String>), String> {
    let mut project = Project {
        export_settings,
        ..Project::default()
    };
    if captions.cues.is_empty() {
        project.blocks[0].narrator = narrator.to_owned();
    } else {
        project.blocks = captions
            .cues
            .into_iter()
            .enumerate()
            .map(|(index, cue)| {
                let mut block = Block::new(index as u64 + 1);
                block.narrator = narrator.to_owned();
                block.text = cue.text;
                block.offset_mode = OffsetMode::Absolute;
                block.offset_seconds = cue.start;
                block
            })
            .collect();
    }
    project.synchronize_sentences()?;
    project.validate()?;
    if serde_json::to_vec(&project)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_BYTES
    {
        return Err("Imported project would exceed 16 MiB".into());
    }
    Ok((project, captions.error))
}

impl EditorApp {
    pub(crate) fn import_srt(&mut self) -> Result<(), String> {
        let Some(path) = rfd::FileDialog::new()
            .set_title(self.tr(
                "開くファイルを選択してください...",
                "Please select the file to open...",
            ))
            .add_filter("SRT", &["srt"])
            .pick_file()
        else {
            return Ok(());
        };
        let captions = read_srt(&path)?;
        let has_cues = !captions.cues.is_empty();
        let narrator = self.default_narrator();
        let (project, error) = replacement(
            captions,
            &narrator,
            self.resources.export_settings.clone().unwrap_or_default(),
        )?;
        self.query_basis_current = false;
        self.invalidate_audio();
        self.project = project;
        self.portrait_state.clear_choices();
        self.filename = None;
        self.selection.clear();
        if !has_cues {
            self.select_only(0);
        }
        self.dirty = has_cues;
        self.undo.clear();
        self.redo.clear();
        self.history_navigation = true;
        self.scroll = 0.0;
        self.error = error.map(|message| {
            let title = self.tr(
                "SRTファイルの読み込みに失敗しました",
                "Failed to load SRT file",
            );
            if message == "Failed to load SRT file" {
                title.to_owned()
            } else {
                format!("{title}\n{message}")
            }
        });
        self.prepare_project();
        Ok(())
    }
}
