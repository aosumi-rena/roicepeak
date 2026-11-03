use super::{Encoding, Format, NamePart, Scope, State, files};
use crate::app::{
    dialog_controls,
    document::Project,
    i18n::{Locale, tr},
    style,
};
use eframe::egui::{self, Color32, Id, Rect, Vec2};
use std::collections::BTreeMap;

fn label(ui: &egui::Ui, rect: Rect, text: &str) {
    style::label(ui, rect.left_center(), text, 14.0, style::TEXT);
}
fn toggle(
    ui: &mut egui::Ui,
    rect: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut bool,
) {
    dialog_controls::toggle(ui, rect, ui.id().with(id), value, "");
}
fn combo<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    rect: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut T,
    choices: &[(T, &str)],
) {
    dialog_controls::combo(ui, rect, ui.id().with(id), value, choices);
}
pub(super) fn settings(
    state: &mut State,
    ctx: &egui::Context,
    project: &Project,
    names: &BTreeMap<String, String>,
    locale: Locale,
) -> bool {
    let mut export = false;
    let open = crate::app::dialogs::viewport(
        ctx,
        "export-settings",
        tr(locale, "出力設定", "Export Settings"),
        [516.0, 708.0],
        |ui| {
            if state.running.is_some() || state.error.is_some() {
                ui.disable();
            }
            let outer = ui.max_rect();
            let rect =
                |x, y, w, h| Rect::from_min_size(outer.min + Vec2::new(x, y), Vec2::new(w, h));
            style::label(
                ui,
                rect(24.0, 24.0, 468.0, 29.0).left_center(),
                tr(locale, "出力", "Export"),
                23.04 / 1.48,
                dialog_controls::HEADING,
            );
            label(
                ui,
                rect(24.0, 65.0, 150.0, 32.0),
                tr(locale, "ファイル名", "File name"),
            );
            dialog_controls::text_field(
                ui,
                rect(192.0, 65.0, 300.0, 32.0),
                Id::new("export-name"),
                &mut state.settings.name,
                "",
                true,
            );
            label(
                ui,
                rect(24.0, 109.0, 150.0, 32.0),
                tr(locale, "フォーマット", "Format"),
            );
            combo(
                ui,
                rect(392.0, 109.0, 100.0, 32.0),
                "export-format",
                &mut state.settings.format,
                &[(Format::Wav, ".wav"), (Format::Flac, ".flac")],
            );
            label(
                ui,
                rect(24.0, 153.0, 150.0, 32.0),
                tr(locale, "サンプルレート", "Sample Rate"),
            );
            combo(
                ui,
                rect(392.0, 153.0, 100.0, 32.0),
                "export-rate",
                &mut state.settings.rate,
                &[
                    (44100, "44100 Hz"),
                    (48000, "48000 Hz"),
                    (96000, "96000 Hz"),
                ],
            );
            let script_label = tr(
                locale,
                "セリフをファイルに保存",
                "Export scripts to file(s)",
            );
            dialog_controls::toggle(
                ui,
                rect(24.0, 217.0, 32.0, 25.0),
                Id::new("export-scripts"),
                &mut state.settings.scripts,
                script_label,
            );
            dialog_controls::section(ui, rect(66.0, 217.0, 426.0, 25.0), script_label);
            ui.add_enabled_ui(state.settings.scripts, |ui| {
                label(
                    ui,
                    rect(24.0, 254.0, 200.0, 32.0),
                    tr(locale, "文字コード", "Character Code"),
                );
                combo(
                    ui,
                    rect(392.0, 254.0, 100.0, 32.0),
                    "export-encoding",
                    &mut state.settings.encoding,
                    &[(Encoding::Utf8, "UTF8"), (Encoding::ShiftJis, "Shift-JIS")],
                );
                for (index, (name, value)) in [
                    ("Text", &mut state.settings.text),
                    ("SRT", &mut state.settings.srt),
                    ("LAB", &mut state.settings.lab),
                ]
                .into_iter()
                .enumerate()
                {
                    let y = 298.0 + index as f32 * 41.0;
                    label(ui, rect(24.0, y, 200.0, 29.0), name);
                    toggle(
                        ui,
                        rect(460.0, y, 32.0, 29.0),
                        ("export-sidecar", index),
                        value,
                    );
                }
            });
            let split_label = if matches!(state.scope, Scope::Project) {
                tr(
                    locale,
                    "ブロックごとに分割して保存",
                    "Export blocks to single files",
                )
            } else {
                tr(locale, "命名規則を有効にする", "Enable Naming Rule")
            };
            dialog_controls::toggle(
                ui,
                rect(24.0, 441.0, 32.0, 25.0),
                Id::new("export-split"),
                &mut state.settings.split,
                split_label,
            );
            dialog_controls::section(ui, rect(66.0, 441.0, 426.0, 25.0), split_label);
            ui.add_enabled_ui(state.settings.split, |ui| {
                label(
                    ui,
                    rect(24.0, 486.0, 100.0, 32.0),
                    tr(locale, "命名規則", "Naming Rule"),
                );
                let options = [
                    (NamePart::None, tr(locale, "なし", "None")),
                    (NamePart::Number, "0, 1, 2 ..."),
                    (NamePart::Voice, tr(locale, "ボイス名", "Voice Name")),
                    (NamePart::Script, tr(locale, "スクリプト", "Script Text")),
                ];
                for index in 0..3 {
                    combo(
                        ui,
                        rect(136.0 + index as f32 * 122.0, 486.0, 112.0, 32.0),
                        ("export-name-part", index),
                        &mut state.settings.parts[index],
                        &options,
                    );
                }
            });
            dialog_controls::section(
                ui,
                rect(24.0, 538.0, 468.0, 25.0),
                tr(locale, "プレビュー", "Preview"),
            );
            let selected = match &state.scope {
                Scope::Project => project.blocks.first(),
                Scope::Blocks(ids) => ids
                    .first()
                    .and_then(|id| project.blocks.iter().find(|block| block.id == *id)),
            };
            let index = if matches!(state.scope, Scope::Blocks(_)) {
                selected
                    .and_then(|b| {
                        project
                            .blocks
                            .iter()
                            .position(|candidate| candidate.id == b.id)
                    })
                    .unwrap_or(0)
            } else {
                0
            };
            let preview = files::filename(
                &state.settings,
                project.blocks.len(),
                index,
                selected
                    .and_then(|b| names.get(&b.narrator.identity).map(String::as_str))
                    .unwrap_or(""),
                selected.map(|b| b.text.as_str()).unwrap_or(""),
                state.settings.split,
            );
            ui.painter()
                .with_clip_rect(rect(24.0, 583.0, 468.0, 32.0))
                .text(
                    rect(24.0, 583.0, 468.0, 32.0).center(),
                    egui::Align2::CENTER_CENTER,
                    preview,
                    style::font(14.0),
                    style::TEXT,
                );
            if style::text_button(
                ui,
                rect(284.0, outer.height() - 56.0, 100.0, 32.0),
                tr(locale, "出力", "Export"),
                true,
                true,
            )
            .clicked()
            {
                export = true;
            }
            if style::text_button(
                ui,
                rect(392.0, outer.height() - 56.0, 100.0, 32.0),
                tr(locale, "キャンセル", "Cancel"),
                false,
                true,
            )
            .clicked()
            {
                state.open = false;
            }
            if state.overwrite.is_some() {
                progress(state, ui.ctx(), locale);
            }
            result(state, ui.ctx(), locale);
        },
    );
    state.open &= open;
    if !open {
        state.cancel();
    }
    export
}
pub(super) fn progress(state: &mut State, ctx: &egui::Context, locale: Locale) {
    if state.overwrite.is_some() {
        let choice = crate::app::menus::native_alert::show(
            ctx,
            Id::new("export-overwrite"),
            "",
            tr(
                locale,
                "同名のファイルが既に存在します。上書きしますか？",
                "A file with the same name already exists. Do you want to overwrite?",
            ),
            crate::app::menus::native_alert::Kind::Information,
            &[tr(locale, "はい", "Yes"), tr(locale, "いいえ", "No")],
        );
        if let Some(choice) = choice {
            if let Some((_, reply)) = state.overwrite.take() {
                let _ = reply.send(choice == 0);
            }
        }
    } else {
        let open = crate::app::dialogs::viewport(
            ctx,
            "export-progress",
            tr(locale, "出力中...", "Exporting..."),
            [320.0, 120.0],
            |ui| {
                let track = Rect::from_min_size(
                    ui.max_rect().min + Vec2::new(20.0, 45.0),
                    Vec2::new(280.0, 30.0),
                );
                ui.painter().rect_filled(track, 15, Color32::from_gray(217));
                let active = Rect::from_min_size(
                    track.min,
                    Vec2::new(track.width() * state.fraction as f32, track.height()),
                );
                ui.painter().with_clip_rect(track).rect_filled(
                    active,
                    15,
                    Color32::from_rgb(73, 85, 148),
                );
                ui.painter().text(
                    track.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{}%", (state.fraction * 100.0).round() as u32),
                    style::font(18.0),
                    style::TEXT,
                );
            },
        );
        if !open {
            state.cancel();
        }
    }
}
pub(super) fn result(state: &mut State, ctx: &egui::Context, locale: Locale) {
    if state.error.is_none() {
        return;
    }
    let message = crate::app::i18n::message(locale, state.error.as_deref().unwrap_or(""));
    if crate::app::menus::native_alert::show(
        ctx,
        Id::new("export-result"),
        tr(locale, "お知らせ", "Notice"),
        &message,
        crate::app::menus::native_alert::Kind::Error,
        &[tr(locale, "閉じる", "Close")],
    )
    .is_some()
    {
        state.error = None;
    }
}
