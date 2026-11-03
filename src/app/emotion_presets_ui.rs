use super::*;
use crate::app::{
    menus::native_alert,
    native_menu::{self, Item},
    style,
};
use eframe::egui::{Color32, Sense, Stroke, StrokeKind, Vec2};

fn choice_order(groups: &[Group]) -> Vec<Option<usize>> {
    let mut order = Vec::new();
    for section in 0..3 {
        let entries = groups
            .iter()
            .enumerate()
            .filter(|(_, group)| match section {
                0 => group.builtin,
                1 => !group.builtin && group.preset.values.voice.is_some(),
                _ => !group.builtin && group.preset.values.voice.is_none(),
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if entries.is_empty() {
            continue;
        }
        if !order.is_empty() {
            order.push(None);
        }
        order.extend(entries.into_iter().map(Some));
    }
    order
}

fn matched_id(app: &EditorApp, state: &State, groups: &[Group]) -> Option<u64> {
    let current = matched(app, state)?;
    groups
        .iter()
        .position(|group| group.builtin == current.builtin && group.preset == current.preset)
        .map(|index| index as u64 + 1)
}

fn user_order(presets: &[Preset]) -> Vec<Option<usize>> {
    let mut order = Vec::new();
    for scalars in [true, false] {
        let entries = presets
            .iter()
            .enumerate()
            .filter(|(_, p)| p.values.voice.is_some() == scalars)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        if entries.is_empty() {
            continue;
        }
        if !order.is_empty() {
            order.push(None);
        }
        order.extend(entries.into_iter().map(Some));
    }
    order
}

pub(super) fn selector(app: &mut EditorApp, ui: &egui::Ui, rect: Rect, color: Color32) {
    let caption = name(app, ui.ctx());
    crate::app::dialog_controls::combo_face_colored(ui, rect, &caption, color);
    let response = ui.interact(rect, ui.id().with("emotion-preset"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), &caption)
    });
    let open = matches!(app.popup, Some(crate::app::editor::Popup::EmotionPreset));
    let mut navigation_groups = None;
    let input = crate::app::dialog_controls::combo_input(ui, &response, open, true, || {
        let state = state(app, ui.ctx());
        let groups = groups(app, &state);
        let order = choice_order(&groups);
        let current = matched_id(app, &state, &groups);
        let rows = order
            .iter()
            .map(|index| crate::app::dialog_controls::ComboRow {
                id: index.map(|i| i as u64 + 1),
                enabled: index.is_some(),
            })
            .collect();
        navigation_groups = Some(groups);
        (current, rows)
    });
    if let Some(id) = input.selected {
        if let Some(group) = navigation_groups
            .as_ref()
            .and_then(|groups| groups.get((id - 1) as usize))
        {
            if let Some(target) = Target::selected(app) {
                apply_for(app, &target, &group.preset.values, false);
            }
        }
    }
    if input.toggle {
        ui.ctx()
            .data_mut(|data| data.insert_temp(Id::new("emotion-preset-anchor"), rect));
        app.toggle(crate::app::editor::Popup::EmotionPreset);
    }
}

pub(super) fn choices(app: &mut EditorApp, ui: &egui::Ui, anchor: Rect) {
    let state = state(app, ui.ctx());
    let groups = groups(app, &state);
    let order = choice_order(&groups);
    let current = matched_id(app, &state, &groups);
    let mut items = Vec::with_capacity(order.len());
    for index in &order {
        match index {
            Some(index) => items.push(Item::CheckedAction {
                text: &groups[*index].label,
                shortcut: String::new(),
                enabled: true,
                checked: current == Some(*index as u64 + 1),
                action: *index,
            }),
            None => items.push(Item::Separator),
        }
    }
    let initial = order
        .iter()
        .position(|index| index.is_some_and(|index| current == Some(index as u64 + 1)));
    let outcome = native_menu::show_selector_at_width(
        ui,
        Id::new("emotion-preset-menu"),
        anchor,
        &items,
        anchor.width(),
        initial,
    );
    if let Some(index) = outcome.action {
        if let Some(target) = Target::selected(app) {
            apply_for(app, &target, &groups[index].preset.values, false);
        }
    }
    if outcome.closed {
        app.popup = None;
    }
}

pub(super) fn actions(app: &mut EditorApp, ui: &egui::Ui) {
    let mut state = state(app, ui.ctx());
    let Some(menu) = state.menu.clone() else {
        native_menu::close(ui.ctx(), Id::new("emotion-preset-actions"));
        return;
    };
    let target = &menu.target;
    if target.correction() && !matches!(app.popup, Some(crate::app::editor::Popup::Emotion)) {
        state.menu = None;
        native_menu::close(ui.ctx(), Id::new("emotion-preset-actions"));
        put(ui.ctx(), state);
        return;
    }
    let valid = capture_for(app, target, true).is_some();
    let current_match = matched_for(app, &state, target);
    let matched = current_match.as_ref().filter(|g| !g.builtin);
    let has_users = !target.presets(&state.store).is_empty();
    let labels = [
        "New Preset",
        "Update Preset",
        "Rename",
        "Delete",
        "Reset",
        "Reset Emotions",
        "Copy Settings",
        "Paste Settings",
    ]
    .map(|key| crate::app::i18n::text(app.locale, key).to_owned());
    let enabled = [
        valid && state.writable,
        valid && state.writable && has_users,
        state.writable && matched.is_some(),
        state.writable && matched.is_some(),
        valid,
        valid,
        valid,
        valid
            && if target.correction() {
                state.correction_copied.is_some()
            } else {
                state.copied.is_some()
            },
    ];
    let groups = groups_for(app, &state, target);
    let headers = ["Presets", "Presets (Emotion only)"]
        .map(|key| crate::app::i18n::text(app.locale, key).to_owned());
    let mut items = Vec::new();
    for index in 0..8 {
        if index == 2 || index == 6 {
            items.push(Item::Separator);
        }
        items.push(Item::Action {
            text: &labels[index],
            shortcut: String::new(),
            enabled: enabled[index],
            action: index,
        });
    }
    if target.correction() {
        for (section, scalars) in [true, false].into_iter().enumerate() {
            let entries: Vec<_> = groups
                .iter()
                .enumerate()
                .filter(|(_, group)| group.preset.values.voice.is_some() == scalars)
                .collect();
            if entries.is_empty() {
                continue;
            }
            items.push(Item::Separator);
            items.push(Item::Header {
                text: &headers[section],
            });
            for (index, group) in entries {
                items.push(Item::CheckedAction {
                    text: &group.label,
                    shortcut: String::new(),
                    enabled: valid,
                    checked: current_match
                        .as_ref()
                        .is_some_and(|current| current.preset.name == group.preset.name),
                    action: 8 + index,
                });
            }
        }
    }
    let outcome = native_menu::show_at(ui, Id::new("emotion-preset-actions"), menu.anchor, &items);
    if outcome.closed {
        state.menu = None;
    }
    if let Some(action) = outcome.action {
        match action {
            0 => state.draft = draft(app, &state, target, Edit::New),
            1 => state.draft = draft(app, &state, target, Edit::Update),
            2 => {
                if let Some(group) = matched.as_ref() {
                    state.draft = draft(app, &state, target, Edit::Rename(group.preset.clone()));
                }
            }
            3 => {
                if let Some(group) = matched.as_ref() {
                    state.draft = draft(app, &state, target, Edit::Delete(group.preset.clone()));
                }
            }
            4 | 5 => apply_for(
                app,
                target,
                &Values {
                    voice: (action == 4).then_some([1.0, 0.0, 1.0, 1.0]),
                    emotions: BTreeMap::new(),
                },
                true,
            ),
            6 => {
                if target.correction() {
                    state.correction_copied = capture_for(app, target, true);
                } else {
                    state.copied = capture_for(app, target, false).map(|values| values.emotions);
                }
            }
            7 => {
                let values = if target.correction() {
                    state.correction_copied.clone()
                } else {
                    state.copied.clone().map(|emotions| Values {
                        voice: None,
                        emotions,
                    })
                };
                if let Some(values) = values {
                    apply_for(app, target, &values, true);
                }
            }
            index => {
                if let Some(group) = groups.get(index - 8) {
                    apply_for(app, target, &group.preset.values, false);
                    state.correction_include_voice = group.preset.values.voice.is_some();
                }
            }
        }
    }
    put(ui.ctx(), state);
}

pub(super) fn dialogs(app: &mut EditorApp, ctx: &egui::Context) {
    let Some(mut state) = ctx.data(|d| d.get_temp::<State>(state_id())) else {
        return;
    };
    if let Some(notice) = state.notice.clone() {
        if native_alert::show(
            ctx,
            Id::new("emotion-preset-notice"),
            crate::app::i18n::text(app.locale, "Notice"),
            &notice.message,
            native_alert::Kind::Information,
            &[crate::app::i18n::text(app.locale, "OK")],
        )
        .is_some()
        {
            state.notice = None;
            state.draft = notice.resume.map(|mut d| {
                d.focus = true;
                d
            });
        }
        put(ctx, state);
        return;
    }
    let Some(mut draft) = state.draft.take() else {
        return;
    };
    let result = match &draft.edit {
        Edit::Delete(_) => native_alert::show(
            ctx,
            Id::new("emotion-preset-dialog"),
            crate::app::i18n::text(app.locale, "Delete"),
            crate::app::i18n::text(app.locale, "Delete this preset?"),
            native_alert::Kind::Information,
            &[
                crate::app::i18n::text(app.locale, "Yes"),
                crate::app::i18n::text(app.locale, "No"),
            ],
        )
        .map(|i| i == 0),
        Edit::Others { .. } => native_alert::show(
            ctx,
            Id::new("emotion-preset-dialog"),
            crate::app::i18n::text(app.locale, "Update Preset"),
            crate::app::i18n::text(app.locale, "Update other blocks using this preset?"),
            native_alert::Kind::Information,
            &[
                crate::app::i18n::text(app.locale, "Yes"),
                crate::app::i18n::text(app.locale, "No"),
            ],
        )
        .map(|i| i == 0),
        _ => edit_dialog(app, ctx, &state, &mut draft),
    };
    match result {
        Some(true) => {
            if let Err(message) = complete(app, &mut state, draft.clone()) {
                let resume = if matches!(draft.edit, Edit::Delete(_) | Edit::Others { .. }) {
                    None
                } else {
                    super::draft(app, &state, &draft.target, draft.edit.clone())
                };
                state.notice = Some(Notice { message, resume });
            }
        }
        Some(false) => (),
        None => state.draft = Some(draft),
    }
    put(ctx, state);
}

fn edit_dialog(
    app: &EditorApp,
    ctx: &egui::Context,
    state: &State,
    draft: &mut Draft,
) -> Option<bool> {
    let id = Id::new("emotion-preset-dialog");
    native_alert::begin(ctx, id);
    let update = matches!(draft.edit, Edit::Update);
    let accept_return = update && draft.target.correction();
    let new = matches!(draft.edit, Edit::New);
    let title = crate::app::i18n::text(
        app.locale,
        if new {
            "Save New Preset"
        } else if update {
            "Update Preset"
        } else {
            "Rename"
        },
    );
    let mut result = None;
    egui::Modal::new(id)
        .backdrop_color(Color32::TRANSPARENT)
        .frame(crate::app::dialog_controls::alert_frame())
        .show(ctx, |ui| {
            let (_, rect) =
                ui.allocate_space(Vec2::new(375.0, if new || update { 208.0 } else { 174.0 }));
            let at = |x, y, w, h| Rect::from_min_size(rect.min + Vec2::new(x, y), Vec2::new(w, h));
            ui.painter().text(
                rect.min + Vec2::new(16.0, 27.0),
                egui::Align2::LEFT_CENTER,
                title,
                style::font(23.04 / 1.48),
                Color32::from_rgb(55, 69, 137),
            );
            let field = at(38.0, 62.0, 300.0, 22.0);
            if update {
                let response = ui.interact(field, id.with("preset"), Sense::click());
                crate::app::dialog_controls::combo_face(ui, field, &draft.name);
                let presets = draft.target.presets(&state.store);
                let mut navigation_order = None;
                let mut open = ui
                    .ctx()
                    .data(|data| data.get_temp::<bool>(id.with("selector")))
                    .unwrap_or(false);
                let input =
                    crate::app::dialog_controls::combo_input(ui, &response, open, true, || {
                        let order = user_order(presets);
                        let current = presets
                            .iter()
                            .position(|p| p.name == draft.name)
                            .map(|i| i as u64 + 1);
                        let rows = order
                            .iter()
                            .map(|index| crate::app::dialog_controls::ComboRow {
                                id: index.map(|i| i as u64 + 1),
                                enabled: index.is_some(),
                            })
                            .collect();
                        navigation_order = Some(order);
                        (current, rows)
                    });
                if let Some(index) = input.selected {
                    if let Some(preset) = presets.get((index - 1) as usize) {
                        draft.name = preset.name.clone();
                    }
                }
                if input.toggle {
                    open = !open;
                    if !open {
                        native_menu::close(ctx, id.with("selector-menu"));
                    }
                }
                if open {
                    let order = navigation_order.unwrap_or_else(|| user_order(presets));
                    let current = presets.iter().position(|p| p.name == draft.name);
                    let items = order
                        .iter()
                        .map(|index| match index {
                            Some(index) => Item::CheckedAction {
                                text: &presets[*index].name,
                                shortcut: String::new(),
                                enabled: true,
                                checked: current == Some(*index),
                                action: *index,
                            },
                            None => Item::Separator,
                        })
                        .collect::<Vec<_>>();
                    let initial = order
                        .iter()
                        .position(|index| *index == current && current.is_some());
                    let outcome = native_menu::show_selector_at_width(
                        ui,
                        id.with("selector-menu"),
                        field,
                        &items,
                        field.width(),
                        initial,
                    );
                    if let Some(index) = outcome.action {
                        draft.name = presets[index].name.clone();
                    }
                    if outcome.closed {
                        open = false;
                    }
                }
                ui.ctx()
                    .data_mut(|data| data.insert_temp(id.with("selector"), open));
            } else {
                let response = ui.put(
                    field,
                    egui::TextEdit::singleline(&mut draft.name)
                        .id(id.with("name"))
                        .return_key(None)
                        .font(style::font(20.88 / 1.48))
                        .text_color(style::TEXT)
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin {
                            left: 4,
                            right: 4,
                            top: 4,
                            bottom: 0,
                        })
                        .vertical_align(egui::Align::Min),
                );
                ui.painter().rect_stroke(
                    field,
                    0,
                    Stroke::new(
                        if response.has_focus() { 2.0 } else { 1.0 },
                        Color32::from_black_alpha(31),
                    ),
                    StrokeKind::Inside,
                );
                if draft.focus {
                    response.request_focus();
                    if let Some(mut edit) = egui::TextEdit::load_state(ctx, id.with("name")) {
                        edit.cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(draft.name.chars().count()),
                            )));
                        edit.store(ctx, id.with("name"));
                    }
                }
            }
            if new || update {
                checkbox(
                    ui,
                    at(38.0, 94.5, 300.0, 25.0),
                    &mut draft.include,
                    crate::app::i18n::text(app.locale, "Contain voice settings"),
                );
            }
            let y = rect.height() - 48.0;
            if native_alert::button(
                ui,
                id.with("ok"),
                at(155.0, y, 100.0, 32.0),
                crate::app::i18n::text(app.locale, "OK"),
                true,
                0xd1374589,
            ) {
                result = Some(true);
            }
            if native_alert::button(
                ui,
                id.with("cancel"),
                at(259.0, y, 100.0, 32.0),
                crate::app::i18n::text(app.locale, "Cancel"),
                false,
                0xd1374589,
            ) {
                result = Some(false);
            }
            if !ctx
                .data(|d| d.get_temp::<bool>(id.with("selector")))
                .unwrap_or(false)
            {
                ui.input_mut(|input| {
                    input.events.retain(|event| match event {
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            pressed: true,
                            modifiers,
                            ..
                        } if accept_return && *modifiers == egui::Modifiers::NONE => {
                            result = Some(true);
                            false
                        }
                        egui::Event::Key {
                            key: egui::Key::Escape,
                            pressed: true,
                            ..
                        } => {
                            result = Some(false);
                            false
                        }
                        _ => true,
                    });
                });
            }
        });
    draft.focus = false;
    if result.is_some() {
        native_menu::close(ctx, id.with("selector-menu"));
        ctx.data_mut(|d| d.remove::<bool>(id.with("selector")));
        native_alert::end(ctx, id);
    }
    result
}

fn checkbox(ui: &mut egui::Ui, rect: Rect, value: &mut bool, label: &str) {
    let response = ui.interact(rect, ui.id().with("include-voice"), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *value, label)
    });
    let box_rect = Rect::from_center_size(rect.min + Vec2::new(16.0, 12.5), Vec2::new(32.0, 16.0));
    ui.painter().rect_filled(
        box_rect,
        8,
        if *value {
            Color32::from_rgb(79, 84, 143)
        } else {
            Color32::from_gray(211)
        },
    );
    ui.painter().circle_filled(
        box_rect.center() + Vec2::new(if *value { 8.0 } else { -8.0 }, 0.0),
        6.0,
        Color32::WHITE,
    );
    style::label(
        ui,
        rect.min + Vec2::new(42.0, 12.5),
        label,
        14.0,
        style::TEXT,
    );
}
