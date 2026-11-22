use crate::app::{
    dialog_controls,
    editor::EditorApp,
    i18n::{self, Locale},
};
use eframe::egui::{self, Color32, Rect};
use roicepeak::speech_frontend::Language;

pub(super) fn show(
    editor: &mut EditorApp,
    ui: &mut egui::Ui,
    rect: Rect,
    selected: &[usize],
    block_id: Option<u64>,
) {
    let Some((&first, rest)) = selected.split_first() else {
        unavailable(ui, rect);
        return;
    };
    let voice = &editor.project.blocks[first].narrator;
    let mut choices = editor.catalog.languages(voice);
    let mut default = choices.first().copied();
    let mut current = Language::from_name(&voice.language);
    for &index in rest {
        let other = &editor.project.blocks[index].narrator;
        if voice.identity != other.identity {
            unavailable(ui, rect);
            return;
        }
        let available = editor.catalog.languages(other);
        if default != available.first().copied() {
            default = None;
        }
        if voice.language != other.language {
            current = None;
        }
        choices.retain(|language| available.contains(language));
    }
    if choices.is_empty() {
        unavailable(ui, rect);
        return;
    }
    let labels: Vec<_> = choices
        .iter()
        .map(|&language| caption(editor.locale, language, Some(language) == default))
        .collect();
    let options: Vec<_> = choices
        .iter()
        .zip(&labels)
        .map(|(&language, caption)| (Some(language), caption.as_str()))
        .collect();
    let color = editor.narrator_color(voice);
    let id = ui.id().with(("voice-language", block_id));
    if dialog_controls::combo_fitted(ui, rect, id, &mut current, &options, color)
        && let Some(language) = current
    {
        editor.begin_edit();
        for &index in selected {
            let block = &mut editor.project.blocks[index];
            if block.narrator.language != language.name() {
                let mut narrator = block.narrator.clone();
                narrator.language = language.name().to_owned();
                block.set_narrator(narrator);
            }
        }
    }
}

fn unavailable(ui: &egui::Ui, rect: Rect) {
    dialog_controls::combo_face_colored(ui, rect, "—", Color32::from_gray(179));
}

fn caption(locale: Locale, language: Language, default: bool) -> String {
    let name = i18n::text(
        locale,
        match language {
            Language::Japanese => "Japanese",
            Language::Mandarin => "Mandarin",
            Language::English => "English",
        },
    );
    if !default {
        return name.to_owned();
    }
    let suffix = match locale {
        Locale::English => "default",
        Locale::Japanese => "既定",
        Locale::SimplifiedChinese => "默认",
        Locale::TraditionalChinese => "預設",
        Locale::Korean => "기본",
        Locale::German => "Standard",
        Locale::Spanish => "por defecto",
        Locale::French => "par défaut",
        Locale::Portuguese => "padrão",
        Locale::Russian => "по умолчанию",
        Locale::Vietnamese => "mặc định",
    };
    format!("{name} ({suffix})")
}
