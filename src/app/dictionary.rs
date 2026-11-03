//! Application-owned user dictionary.
use crate::app::{editor::EditorApp, style};
#[path = "dictionary_controls.rs"]
mod controls;
#[path = "dictionary_draft.rs"]
mod draft;
#[path = "dictionary_file.rs"]
mod file;
#[path = "dictionary_io_ui.rs"]
mod io_ui;
#[path = "dictionary_record.rs"]
mod record;
#[path = "dictionary_state.rs"]
mod state;
#[path = "dictionary_store.rs"]
mod store;
use eframe::egui::{self, Color32, Rect, Vec2};
use serde::{Deserialize, Serialize};
pub(crate) use state::State;
pub(crate) use store::{Snapshot, Store};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub word: String,
    pub reading: String,
    pub part_of_speech: usize,
    pub priority: u8,
    pub accent: usize,
    pub native: record::Native,
}
impl Default for Entry {
    fn default() -> Self {
        Self {
            word: String::new(),
            reading: String::new(),
            part_of_speech: 0,
            priority: 5,
            accent: 0,
            native: record::Native::default(),
        }
    }
}
#[derive(Default)]
pub struct Editor {
    pub query: String,
    pub selected: Option<usize>,
    pub draft: Option<Entry>,
    pub editing: Option<usize>,
    pub validation: Option<String>,
    draft_state: draft::State,
    import_path: Option<std::path::PathBuf>,
    file_error: Option<String>,
}
impl Editor {
    pub(crate) fn close_dialog(&mut self) {
        self.draft = None;
        self.draft_state = draft::State::default();
        self.import_path = None;
        self.file_error = None;
    }
    pub(crate) fn begin_draft(&mut self, entry: Entry, editing: Option<usize>) {
        self.draft_state.begin(&entry);
        self.draft = Some(entry);
        self.editing = editing;
        self.validation = None;
    }
}

pub(crate) fn inspect_native(
    bytes: &[u8],
    previous: Option<&[u8]>,
) -> Result<serde_json::Value, String> {
    let incoming = file::parse(bytes)?;
    let entries = if let Some(previous) = previous {
        file::merge(&file::parse(previous)?, &incoming)?
    } else {
        incoming
    };
    let accent_features = entries
        .iter()
        .map(|entry| {
            if entry.native.opaque {
                return serde_json::Value::Null;
            }
            let word = roicepeak::frontend::UserWord {
                spelling: entry.word.clone(),
                reading: entry.reading.clone(),
                accent: entry.accent,
                priority: entry.priority,
                part_of_speech: roicepeak::frontend::UserPartOfSpeech::try_from(
                    entry.part_of_speech,
                )
                .unwrap(),
                overwrite_accents: entry.native.overwrite_accents.clone(),
            };
            match word.accent_feature() {
                Ok(feature) => serde_json::json!({"feature":feature}),
                Err(error) => serde_json::json!({"error":error.to_string()}),
            }
        })
        .collect::<Vec<_>>();
    let exported = String::from_utf8(file::encode(&entries)?).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"entries":entries,"exported":exported,"accent_features":accent_features}))
}

const POS: [&str; 6] = [
    "普通名詞",
    "固有名詞:一般",
    "固有名詞:人名",
    "固有名詞:姓",
    "固有名詞:名",
    "固有名詞:地域",
];
impl EditorApp {
    pub fn dictionary_content(&mut self, ui: &mut egui::Ui) {
        let dictionary = self.dictionary_state.display_snapshot();
        let writable = self.dictionary_writable();
        let o = ui.max_rect().min;
        let left = Rect::from_min_size(o, Vec2::new(240.0, 440.0));
        ui.painter().rect_filled(left, 0, Color32::from_gray(240));
        ui.painter().line_segment(
            [left.right_top(), left.right_bottom()],
            egui::Stroke::new(1.0, style::BORDER),
        );
        controls::search(ui, o, &mut self.dictionary_ui.query, self.locale);
        let more = Rect::from_min_size(o + Vec2::new(200.0, 16.0), Vec2::splat(32.0));
        self.dictionary_more(ui, more);
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(Rect::from_min_size(
                o + Vec2::new(16.0, 56.0),
                Vec2::new(208.0, 328.0),
            )),
            |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, entry) in dictionary.entries().iter().enumerate() {
                        if !entry.word.contains(&self.dictionary_ui.query)
                            && !entry.reading.contains(&self.dictionary_ui.query)
                        {
                            continue;
                        }
                        if controls::entry_row(
                            ui,
                            i,
                            if entry.native.opaque {
                                crate::app::i18n::tr(
                                    self.locale,
                                    "(無効なエントリー)",
                                    "(Invalid Entry)",
                                )
                            } else {
                                &entry.word
                            },
                            self.dictionary_ui.selected == Some(i),
                        )
                        .clicked()
                        {
                            self.dictionary_ui.selected = Some(i);
                        }
                    }
                });
            },
        );
        if style::text_button(
            ui,
            Rect::from_min_size(o + Vec2::new(16.0, 392.0), Vec2::new(208.0, 32.0)),
            self.tr("単語の追加", "Add word"),
            true,
            writable,
        )
        .clicked()
        {
            self.dictionary_ui.begin_draft(Entry::default(), None);
        }
        if let Some(index) = self
            .dictionary_ui
            .selected
            .filter(|i| *i < dictionary.entries().len())
        {
            let entry = &dictionary.entries()[index];
            style::label(
                ui,
                o + Vec2::new(264.0, 40.0),
                &entry.word,
                20.0,
                style::TEXT,
            );
            style::label(
                ui,
                o + Vec2::new(264.0, 82.0),
                &entry.reading,
                16.0,
                style::TEXT,
            );
            style::label(
                ui,
                o + Vec2::new(264.0, 124.0),
                crate::app::i18n::text(self.locale, POS[entry.part_of_speech.min(5)]),
                14.0,
                style::TEXT,
            );
            style::label(
                ui,
                o + Vec2::new(264.0, 162.0),
                crate::app::i18n::format(
                    self.locale,
                    "Priority {}",
                    &[&entry.priority.to_string()],
                ),
                14.0,
                style::TEXT,
            );
            if style::text_button(
                ui,
                Rect::from_min_size(o + Vec2::new(264.0, 210.0), Vec2::new(120.0, 32.0)),
                self.tr("編集...", "Edit..."),
                false,
                writable,
            )
            .clicked()
            {
                self.dictionary_ui.begin_draft(entry.clone(), Some(index));
            }
            if style::text_button(
                ui,
                Rect::from_min_size(o + Vec2::new(400.0, 210.0), Vec2::new(120.0, 32.0)),
                self.tr("削除", "Remove"),
                false,
                writable,
            )
            .clicked()
            {
                let mut entries = dictionary.entries().to_vec();
                entries.remove(index);
                match self.commit_dictionary(&dictionary, entries) {
                    Ok(()) => self.dictionary_ui.selected = None,
                    Err(error) => self.dictionary_ui.file_error = Some(error),
                }
            }
        }
    }
    pub fn dictionary_draft(&mut self, ui: &mut egui::Ui) -> bool {
        draft::show(self, ui)
    }
}
