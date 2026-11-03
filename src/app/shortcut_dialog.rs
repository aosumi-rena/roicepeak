//! Named shortcut slots and their editor drafts.
use crate::app::{
    editor::EditorApp,
    menus::native_alert,
    native_menu,
    shortcuts::{Chord, ShortcutMap, Slot},
};
use eframe::egui::{self, Id, Rect};
use std::io::Read;
#[path = "shortcut_dialog_ui.rs"]
mod ui;

#[derive(Clone)]
struct Draft {
    slot: Slot,
    chord: Option<Chord>,
}
#[derive(Clone, Copy)]
enum Operation {
    Load,
    Reset,
}
#[derive(Clone, Copy)]
enum Confirmation {
    Overwrite(Operation),
    Default(Slot),
}

pub(crate) struct State {
    opened: bool,
    search: String,
    expanded: [bool; 3],
    draft: Option<Draft>,
    header: Option<Rect>,
    confirmation: Option<Confirmation>,
    refocus: Option<Slot>,
    previous_focus: Option<Id>,
    field_rect: Option<Rect>,
    reset_hover: Option<Slot>,
    notice: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            opened: false,
            search: String::new(),
            expanded: [true; 3],
            draft: None,
            header: None,
            confirmation: None,
            refocus: None,
            previous_focus: None,
            field_rect: None,
            reset_hover: None,
            notice: None,
        }
    }
}
fn id() -> Id {
    Id::new("shortcut-settings")
}
fn field_id(slot: Slot) -> Id {
    id().with(("key", slot))
}

impl State {
    pub(crate) fn capture_key(
        &mut self,
        ctx: &egui::Context,
        map: &ShortcutMap,
        chord: Chord,
    ) -> bool {
        if self.confirmation.is_some() || self.notice.is_some() || self.header.is_some() {
            return false;
        }
        let Some(draft) = &mut self.draft else {
            return false;
        };
        if !ctx.memory(|memory| memory.has_focus(field_id(draft.slot))) {
            return false;
        }
        draft.chord = Some(chord).filter(|chord| {
            !matches!(
                chord.key,
                crate::app::shortcuts::Key::Backspace | crate::app::shortcuts::Key::Delete
            )
        });
        if !map.conflicts(draft.slot, draft.chord).is_empty() {
            self.refocus = Some(draft.slot);
        }
        ctx.request_repaint();
        true
    }
    fn select(&mut self, map: &mut ShortcutMap, slot: Slot) {
        if self.draft.as_ref().is_some_and(|draft| draft.slot == slot) {
            return;
        }
        self.commit(map);
        self.draft = Some(Draft {
            slot,
            chord: map.chord(slot),
        });
    }
    fn commit(&mut self, map: &mut ShortcutMap) {
        if let Some(draft) = self.draft.take() {
            // A conflict never replaces another slot or mutates the stored value.
            let _ = map.assign(draft.slot, draft.chord);
        }
        self.field_rect = None;
    }
    fn cancel_draft(&mut self, map: &ShortcutMap) {
        if let Some(draft) = &mut self.draft {
            draft.chord = map.chord(draft.slot);
            self.refocus = Some(draft.slot);
        }
    }
    fn default_draft(&mut self, slot: Slot) {
        self.draft = Some(Draft {
            slot,
            chord: slot.default_chord(),
        });
        self.refocus = Some(slot);
    }
    pub(crate) fn show(&mut self, app: &mut EditorApp, ctx: &egui::Context) {
        if !app.shortcut_settings {
            if self.opened {
                let before = app.shortcuts.clone();
                self.close(app, ctx);
                if app.shortcuts != before {
                    if let Err(error) = app.shortcuts.save_local() {
                        app.shortcuts = before;
                        app.error = Some(error);
                    }
                }
            }
            return;
        }
        let first = !self.opened;
        if first {
            self.opened = true;
            self.previous_focus = ctx.memory(|memory| memory.focused());
            ctx.memory_mut(|memory| memory.request_focus(id().with("keyboard")));
        }
        if self.confirmation.is_some() {
            native_alert::begin(ctx, id().with("confirm"));
        }
        let before = app.shortcuts.clone();
        ui::show(self, app, ctx, first);
        self.confirm(app, ctx);
        if let Some(notice) = self.notice.clone() {
            if native_alert::show(
                ctx,
                id().with("notice"),
                crate::app::i18n::text(app.locale, "Notice"),
                &notice,
                native_alert::Kind::Information,
                &[crate::app::i18n::text(app.locale, "OK")],
            )
            .is_some()
            {
                self.notice = None;
            }
        }
        if !app.shortcut_settings {
            self.close(app, ctx);
        }
        if app.shortcuts != before {
            if let Err(error) = app.shortcuts.save_local() {
                app.shortcuts = before;
                if app.shortcut_settings {
                    self.notice = Some(error);
                } else {
                    app.error = Some(error);
                }
            }
        }
    }
    fn close(&mut self, app: &mut EditorApp, ctx: &egui::Context) {
        self.commit(&mut app.shortcuts);
        native_menu::close(ctx, id().with("menu"));
        if let Some(focus) = self.previous_focus {
            ctx.memory_mut(|memory| memory.request_focus(focus));
        }
        *self = Self::default();
    }
    fn confirm(&mut self, app: &mut EditorApp, ctx: &egui::Context) {
        let Some(confirmation) = self.confirmation else {
            return;
        };
        let default = matches!(confirmation, Confirmation::Default(_));
        let labels: Vec<_> = if default {
            vec!["Yes", "No"]
        } else {
            vec!["Yes", "No", "Cancel"]
        }
        .into_iter()
        .map(|key| crate::app::i18n::text(app.locale, key))
        .collect();
        let message = if default {
            "Restore the default shortcut?"
        } else {
            "Save current shortcuts before replacing them?"
        };
        let Some(result) = native_alert::show(
            ctx,
            id().with("confirm"),
            crate::app::i18n::text(
                app.locale,
                if default {
                    "Keyboard Shortcuts..."
                } else {
                    "Keyboard Shortcuts"
                },
            ),
            crate::app::i18n::text(app.locale, message),
            native_alert::Kind::Information,
            &labels,
        ) else {
            return;
        };
        self.confirmation = None;
        match confirmation {
            Confirmation::Default(slot) => {
                if result == 0 {
                    self.default_draft(slot);
                } else {
                    self.refocus = Some(slot);
                }
            }
            Confirmation::Overwrite(operation) => {
                if result >= 2 {
                    return;
                }
                self.commit(&mut app.shortcuts);
                if result == 0 {
                    match save(&app.shortcuts, app.locale) {
                        Ok(true) => (),
                        Ok(false) => return,
                        Err(error) => {
                            self.notice = Some(error);
                            return;
                        }
                    }
                }
                match operation {
                    Operation::Load => match load(app.locale) {
                        Ok(Some(map)) => app.shortcuts = map,
                        Ok(None) => (),
                        Err(error) => self.notice = Some(error),
                    },
                    Operation::Reset => app.shortcuts.reset(),
                }
            }
        }
    }
}
fn load(locale: crate::app::i18n::Locale) -> Result<Option<ShortcutMap>, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_title(crate::app::i18n::text(locale, "Load Shortcut Set"))
        .add_filter("ROICEPEAK shortcuts", &["json"])
        .pick_file()
    else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65_536 {
        return Err("Shortcut file exceeds 64 KiB".into());
    }
    ShortcutMap::decode(&bytes).map(Some)
}
fn save(map: &ShortcutMap, locale: crate::app::i18n::Locale) -> Result<bool, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_title(crate::app::i18n::text(locale, "Save Shortcut Set"))
        .add_filter("ROICEPEAK shortcuts", &["json"])
        .set_file_name("ROICEPEAK-shortcuts.json")
        .save_file()
    else {
        return Ok(false);
    };
    let bytes = map.encode()?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())?;
    Ok(true)
}
