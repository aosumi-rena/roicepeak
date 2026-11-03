use eframe::egui::{self, Response};

pub(crate) struct Row {
    pub id: Option<u64>,
    pub enabled: bool,
}

#[derive(Default)]
pub(crate) struct Input {
    pub toggle: bool,
    pub selected: Option<u64>,
}

pub(crate) fn interact(
    ui: &egui::Ui,
    response: &Response,
    popup_open: bool,
    enabled: bool,
    options: impl FnOnce() -> (Option<u64>, Vec<Row>),
) -> Input {
    if !enabled
        || !ui.is_enabled()
        || !response.enabled()
        || !ui
            .ctx()
            .memory(|memory| memory.allows_interaction(ui.layer_id()))
    {
        return Input::default();
    }
    let keyboard_click = ui.input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::Enter | egui::Key::Space,
                    pressed: true,
                    ..
                }
            )
        })
    });
    let pointer = response.clicked_by(egui::PointerButton::Primary);
    let mut result = Input {
        toggle: pointer || (response.clicked() && !keyboard_click),
        selected: None,
    };
    if pointer {
        response.request_focus();
    }
    if popup_open || !response.has_focus() {
        return result;
    }
    ui.ctx().memory_mut(|memory| {
        memory.set_focus_lock_filter(
            response.id,
            egui::EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..Default::default()
            },
        )
    });
    let keys = ui.input_mut(|input| {
        let mut keys = Vec::new();
        let mut opened = false;
        input.events.retain(|event| {
            if opened {
                return true;
            }
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
                && *modifiers == egui::Modifiers::NONE
                && matches!(
                    key,
                    egui::Key::ArrowUp
                        | egui::Key::ArrowDown
                        | egui::Key::ArrowLeft
                        | egui::Key::ArrowRight
                        | egui::Key::Enter
                )
            {
                keys.push(*key);
                opened = *key == egui::Key::Enter;
                return false;
            }
            true
        });
        keys
    });
    let navigation = keys.iter().any(|key| *key != egui::Key::Enter);
    let (initial, rows) = if navigation {
        options()
    } else {
        (None, Vec::new())
    };
    let initial = initial.filter(|id| *id != 0);
    let mut selected = initial;
    for key in keys {
        if key == egui::Key::Enter {
            result.toggle = true;
            break;
        }
        let backwards = matches!(key, egui::Key::ArrowUp | egui::Key::ArrowLeft);
        let mut index = selected
            .and_then(|id| rows.iter().position(|row| row.id == Some(id)))
            .map_or(-1, |index| index as isize);
        loop {
            index += if backwards { -1 } else { 1 };
            if index < 0 || index >= rows.len() as isize {
                break;
            }
            let row = &rows[index as usize];
            if row.enabled && row.id.is_some_and(|id| id != 0) {
                selected = row.id;
                break;
            }
        }
    }
    if selected != initial {
        result.selected = selected;
    }
    result
}
