use crate::app::{
    editor::EditorApp,
    i18n::{self, Locale},
    native_menu::{self, Item},
    shortcuts::{Command, ShortcutMap},
};
use eframe::egui::{self, Id, Rect, Vec2};

#[derive(Clone, Copy)]
enum Action {
    Command(Command),
    Recent(usize),
}

fn id() -> Id {
    Id::new("native-file-menu")
}

pub(crate) fn close(ctx: &egui::Context) {
    native_menu::close(ctx, id());
}

fn entry(
    locale: Locale,
    shortcuts: &ShortcutMap,
    command: Command,
    japanese: &'static str,
    english: &'static str,
    enabled: bool,
) -> Item<'static, Action> {
    Item::Action {
        text: i18n::tr(locale, japanese, english),
        shortcut: shortcuts
            .primary_chord(command)
            .map(|chord| chord.label())
            .unwrap_or_default(),
        enabled,
        action: Action::Command(command),
    }
}

impl EditorApp {
    pub(crate) fn file_menu(&mut self, ui: &egui::Ui, ctx: &egui::Context, screen: Rect) {
        use Command::*;
        let locale = self.locale;
        let command = |kind, ja, en, enabled| entry(locale, &self.shortcuts, kind, ja, en, enabled);
        let recent_labels: Vec<_> = self
            .recent_projects
            .paths()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        let recent_items = recent_labels
            .iter()
            .enumerate()
            .rev()
            .map(|(index, text)| Item::Action {
                text,
                shortcut: String::new(),
                enabled: true,
                action: Action::Recent(index),
            })
            .collect();
        let items = vec![
            command(NewProject, "新規プロジェクト", "New Project", true),
            command(OpenProject, "ファイルを開く...", "Open File...", true),
            Item::Submenu {
                text: i18n::tr(locale, "インポート", "Import"),
                enabled: true,
                items: vec![
                    command(
                        ImportText,
                        "テキストファイルを開く...",
                        "Import Text file...",
                        true,
                    ),
                    command(
                        ImportSrt,
                        "SRTファイルを開く...",
                        "Import SRT file...",
                        true,
                    ),
                    command(
                        ImportCsv,
                        "CSVファイルを開く...",
                        "Import CSV file...",
                        true,
                    ),
                    command(
                        ImportSsml,
                        "SSMLファイルを開く...",
                        "Import SSML file...",
                        true,
                    ),
                ],
            },
            Item::Submenu {
                text: i18n::tr(locale, "最近使用したファイル...", "Open Recent..."),
                enabled: !recent_labels.is_empty(),
                items: recent_items,
            },
            Item::Separator,
            command(SaveProject, "プロジェクトの保存", "Save Project", true),
            command(
                SaveProjectAs,
                "名前を付けてプロジェクトを保存...",
                "Save As...",
                true,
            ),
            Item::Separator,
            command(
                Export,
                "出力",
                "Export",
                self.project
                    .blocks
                    .iter()
                    .any(|block| !block.text.trim().is_empty()),
            ),
            Item::Separator,
            command(
                ReplaceVoices,
                "ボイスの置換",
                "Replace Voices",
                !self.catalog.voices.is_empty() && !self.project.blocks.is_empty(),
            ),
            Item::Separator,
            command(Preferences, "環境設定...", "Preferences...", true),
            command(
                KeyboardShortcuts,
                "キー設定...",
                "Keyboard Shortcuts...",
                true,
            ),
            command(
                CheckForUpdates,
                "更新の確認...",
                "Check for updates...",
                false,
            ),
            command(About, "バージョン情報", "About", true),
        ];
        let anchor = Rect::from_min_size(
            screen.min + Vec2::new(crate::app::toolbar::origin(screen.width()), 6.0),
            Vec2::splat(32.0),
        );
        let result = native_menu::show_at(ui, id(), anchor, &items);
        if result.closed {
            self.popup = None;
        }
        match result.action {
            Some(Action::Command(command)) => self.command(command, ctx),
            Some(Action::Recent(index)) => {
                if let Some(path) = self.recent_projects.paths().get(index).cloned() {
                    self.request_action(crate::app::editor::PendingAction::OpenPath(path), ctx);
                }
            }
            None => (),
        }
    }
}
