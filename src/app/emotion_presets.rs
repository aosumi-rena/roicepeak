//! Narrator-scoped preset operations and transient editor state.
#[path = "emotion_presets_ui.rs"]
mod ui;
use crate::app::{
    document::Block,
    editor::EditorApp,
    preset_store::{Preset, Store, Values},
};
use eframe::egui::{self, Id, Rect};
use std::{collections::BTreeMap, sync::Arc};

fn state_id() -> Id {
    Id::new("emotion-preset-state")
}

#[derive(Clone)]
pub(super) struct Group {
    pub preset: Preset,
    pub label: String,
    pub builtin: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct Target {
    identity: String,
    block: Option<u64>,
}

impl Target {
    fn selected(app: &EditorApp) -> Option<Self> {
        let block = selected(app)?;
        if app
            .selected_indices()
            .iter()
            .any(|&index| app.project.blocks[index].narrator.identity != block.narrator.identity)
        {
            return None;
        }
        Some(Self {
            identity: block.narrator.identity.clone(),
            block: Some(block.id),
        })
    }
    fn correction(&self) -> bool {
        self.block.is_none()
    }
    fn presets<'a>(&self, store: &'a Store) -> &'a [Preset] {
        self.map(store)
            .get(&self.identity)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    fn map<'a>(&self, store: &'a Store) -> &'a BTreeMap<String, Vec<Preset>> {
        if self.correction() {
            &store.corrections
        } else {
            &store.narrators
        }
    }
    fn map_mut<'a>(&self, store: &'a mut Store) -> &'a mut BTreeMap<String, Vec<Preset>> {
        if self.correction() {
            &mut store.corrections
        } else {
            &mut store.narrators
        }
    }
}

#[derive(Clone)]
struct Menu {
    anchor: Rect,
    target: Target,
}

#[derive(Clone)]
enum Edit {
    New,
    Rename(Preset),
    Update,
    Delete(Preset),
    Others { values: Values, blocks: Vec<u64> },
}

#[derive(Clone)]
struct Draft {
    target: Target,
    name: String,
    include: bool,
    edit: Edit,
    focus: bool,
}

#[derive(Clone)]
struct Notice {
    message: String,
    resume: Option<Draft>,
}

#[derive(Clone)]
struct State {
    store: Arc<Store>,
    writable: bool,
    menu: Option<Menu>,
    draft: Option<Draft>,
    notice: Option<Notice>,
    copied: Option<BTreeMap<String, f32>>,
    correction_copied: Option<Values>,
    include_voice: bool,
    correction_include_voice: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            store: Arc::new(Store::default()),
            writable: false,
            menu: None,
            draft: None,
            notice: None,
            copied: None,
            correction_copied: None,
            include_voice: true,
            correction_include_voice: true,
        }
    }
}

fn state(_app: &mut EditorApp, ctx: &egui::Context) -> State {
    if let Some(state) = ctx.data(|d| d.get_temp::<State>(state_id())) {
        return state;
    }
    let state = match Store::load() {
        Ok(store) => State {
            store: Arc::new(store),
            writable: true,
            ..Default::default()
        },
        Err(error) => State {
            notice: Some(Notice {
                message: error,
                resume: None,
            }),
            ..Default::default()
        },
    };
    put(ctx, state.clone());
    state
}

fn put(ctx: &egui::Context, state: State) {
    ctx.data_mut(|d| d.insert_temp(state_id(), state));
}

fn selected(app: &EditorApp) -> Option<&Block> {
    app.selected().and_then(|i| app.project.blocks.get(i))
}

fn capture(app: &EditorApp, block: &Block, include: bool) -> Values {
    Values {
        voice: include.then_some([
            block.parameters.speed.normalized(),
            block.parameters.pitch.normalized(),
            block.parameters.pause.normalized(),
            block.parameters.volume.normalized(),
        ]),
        emotions: app
            .narrator_emotion_controls(&block.narrator)
            .iter()
            .map(|control| {
                (
                    control.key.clone(),
                    block
                        .parameters
                        .emotions
                        .get(&control.key)
                        .copied()
                        .unwrap_or_default()
                        .normalized(),
                )
            })
            .collect(),
    }
}

fn groups(app: &EditorApp, state: &State) -> Vec<Group> {
    let Some(target) = Target::selected(app) else {
        return Vec::new();
    };
    groups_for(app, state, &target)
}

fn capture_for(app: &EditorApp, target: &Target, include: bool) -> Option<Values> {
    if let Some(id) = target.block {
        if Target::selected(app)?.identity != target.identity {
            return None;
        }
        let block = app
            .project
            .blocks
            .iter()
            .find(|block| block.id == id && block.narrator.identity == target.identity)?;
        return Some(capture(app, block, include));
    }
    let metadata = app.catalog.identity_metadata(&target.identity)?;
    let parameters = app
        .project
        .narrator_settings
        .get(&target.identity)
        .cloned()
        .unwrap_or_default();
    Some(Values {
        voice: include.then_some([
            parameters.speed.normalized(),
            parameters.pitch.normalized(),
            parameters.pause.normalized(),
            parameters.volume.normalized(),
        ]),
        emotions: metadata
            .preset_controls
            .iter()
            .map(|control| {
                (
                    control.key.clone(),
                    parameters
                        .emotions
                        .get(&control.key)
                        .copied()
                        .unwrap_or_default()
                        .normalized(),
                )
            })
            .collect(),
    })
}

fn groups_for(app: &EditorApp, state: &State, target: &Target) -> Vec<Group> {
    let Some(metadata) = app.catalog.identity_metadata(&target.identity) else {
        return Vec::new();
    };
    let controls = &metadata.preset_controls;
    let mut groups: Vec<_> = controls
        .iter()
        .map(|chosen| Group {
            preset: Preset {
                name: chosen.key.clone(),
                values: Values {
                    voice: None,
                    emotions: controls
                        .iter()
                        .map(|control| {
                            (
                                control.key.clone(),
                                if control.key == chosen.key { 1.0 } else { 0.0 },
                            )
                        })
                        .collect(),
                },
            },
            label: crate::app::i18n::text(app.locale, &chosen.label).into(),
            builtin: true,
        })
        .collect();
    groups.extend(
        target
            .presets(&state.store)
            .iter()
            .cloned()
            .map(|preset| Group {
                label: preset.name.clone(),
                preset,
                builtin: false,
            }),
    );
    groups
}

fn matches(values: &Values, current: &Values, skip_missing: bool) -> bool {
    (values.voice.is_none() || values.voice == current.voice)
        && values.emotions.iter().all(|(key, value)| {
            current
                .emotions
                .get(key)
                .map_or(skip_missing && values.voice.is_none(), |v| v == value)
        })
}

fn matched(app: &EditorApp, state: &State) -> Option<Group> {
    matched_for(app, state, &Target::selected(app)?)
}

fn selected_emotions_mixed(app: &EditorApp, target: &Target) -> bool {
    if target.correction() {
        return false;
    }
    let Some(metadata) = app.catalog.identity_metadata(&target.identity) else {
        return false;
    };
    let indices = app.selected_indices();
    let Some(&first) = indices.first() else {
        return false;
    };
    metadata.preset_controls.iter().any(|control| {
        let value = app.project.blocks[first]
            .parameters
            .emotions
            .get(&control.key)
            .copied()
            .unwrap_or_default();
        indices.iter().any(|&index| {
            app.project.blocks[index]
                .parameters
                .emotions
                .get(&control.key)
                .copied()
                .unwrap_or_default()
                != value
        })
    })
}

fn matched_for(app: &EditorApp, state: &State, target: &Target) -> Option<Group> {
    if selected_emotions_mixed(app, target) {
        return None;
    }
    let current = capture_for(app, target, true)?;
    let groups = groups_for(app, state, target);
    for scalars in [true, false] {
        if let Some(group) = groups.iter().find(|g| {
            g.preset.values.voice.is_some() == scalars && matches(&g.preset.values, &current, false)
        }) {
            return Some(group.clone());
        }
    }
    None
}

pub(crate) fn selector(app: &mut EditorApp, ui: &egui::Ui, rect: Rect, color: egui::Color32) {
    ui::selector(app, ui, rect, color);
}

pub(crate) fn name(app: &mut EditorApp, ctx: &egui::Context) -> String {
    let Some(target) = Target::selected(app) else {
        return crate::app::i18n::text(app.locale, "(no choices)").into();
    };
    if selected_emotions_mixed(app, &target) {
        return crate::app::i18n::text(app.locale, "Multiple Values").into();
    }
    let mut state = state(app, ctx);
    if let Some(group) = matched(app, &state) {
        if state.include_voice != group.preset.values.voice.is_some() {
            state.include_voice = group.preset.values.voice.is_some();
            put(ctx, state);
        }
        group.label
    } else {
        crate::app::i18n::text(app.locale, "Custom settings").into()
    }
}

fn apply(app: &mut EditorApp, block_id: u64, values: &Values, zero_missing: bool) {
    let Some(index) = app.project.blocks.iter().position(|b| b.id == block_id) else {
        return;
    };
    let controls = app.narrator_emotion_controls(&app.project.blocks[index].narrator);
    let parameters = &app.project.blocks[index].parameters;
    let scalar_change = values.voice.is_some_and(|[speed, pitch, pause, volume]| {
        [
            parameters.speed.normalized(),
            parameters.pitch.normalized(),
            parameters.pause.normalized(),
            parameters.volume.normalized(),
        ] != [speed, pitch, pause, volume]
    });
    let emotion_change = controls.iter().any(|control| {
        values
            .emotions
            .get(&control.key)
            .copied()
            .or(zero_missing.then_some(0.0))
            .is_some_and(|value| {
                parameters
                    .emotions
                    .get(&control.key)
                    .copied()
                    .unwrap_or_default()
                    .normalized()
                    != value
            })
    });
    if !scalar_change && !emotion_change {
        return;
    }
    app.begin_edit();
    let block = &mut app.project.blocks[index];
    if let Some([speed, pitch, pause, volume]) = values.voice {
        block.parameters.speed = crate::app::document::ControlValue::from_normalized(speed);
        block.parameters.pitch = crate::app::document::ControlValue::from_normalized(pitch);
        block.parameters.pause = crate::app::document::ControlValue::from_normalized(pause);
        block.parameters.volume = crate::app::document::ControlValue::from_normalized(volume);
    }
    for control in controls {
        let value = values
            .emotions
            .get(&control.key)
            .copied()
            .or(zero_missing.then_some(0.0));
        if let Some(value) = value {
            let value = crate::app::document::ControlValue::from_normalized(value);
            if block
                .parameters
                .emotions
                .get(&control.key)
                .copied()
                .unwrap_or_default()
                != value
            {
                app.portrait_state.note_block_change(
                    block.id,
                    &block.narrator.identity,
                    &control.key,
                );
                block.parameters.emotions.insert(control.key, value);
            }
        }
    }
}

pub(crate) fn open_actions(app: &mut EditorApp, ctx: &egui::Context, anchor: Rect) {
    let Some(target) = Target::selected(app) else {
        return;
    };
    open_menu(app, ctx, Menu { anchor, target }, true);
}

pub(crate) fn open_correction_actions(
    app: &mut EditorApp,
    ctx: &egui::Context,
    identity: &str,
    anchor: Rect,
) {
    if app.catalog.identity_metadata(identity).is_none() {
        return;
    }
    open_menu(
        app,
        ctx,
        Menu {
            anchor,
            target: Target {
                identity: identity.to_owned(),
                block: None,
            },
        },
        false,
    );
}

fn open_menu(app: &mut EditorApp, ctx: &egui::Context, menu: Menu, close_parent: bool) {
    let mut state = state(app, ctx);
    if let Some(group) = matched_for(app, &state, &menu.target) {
        if menu.target.correction() {
            state.correction_include_voice = group.preset.values.voice.is_some();
        } else {
            state.include_voice = group.preset.values.voice.is_some();
        }
    }
    state.menu = if state
        .menu
        .as_ref()
        .is_some_and(|old| old.target == menu.target)
    {
        None
    } else {
        Some(menu)
    };
    if close_parent {
        app.popup = None;
    }
    put(ctx, state);
}

fn apply_for(app: &mut EditorApp, target: &Target, values: &Values, zero_missing: bool) {
    if values.voice.is_some_and(|voice| voice[3] < 0.0) {
        app.error = Some("Volume cannot be negative".into());
        return;
    }
    if let Some(id) = target.block {
        if app
            .project
            .blocks
            .iter()
            .any(|b| b.id == id && b.narrator.identity == target.identity)
        {
            let indices = app.selected_indices();
            if indices
                .iter()
                .any(|&index| app.project.blocks[index].narrator.identity != target.identity)
            {
                return;
            }
            let ids = indices
                .into_iter()
                .map(|index| app.project.blocks[index].id)
                .collect::<Vec<_>>();
            for id in ids {
                apply(app, id, values, zero_missing);
            }
        }
        return;
    }
    let Some(metadata) = app.catalog.identity_metadata(&target.identity) else {
        return;
    };
    let keys: Vec<_> = metadata
        .preset_controls
        .iter()
        .map(|control| control.key.clone())
        .collect();
    let mut parameters = app
        .project
        .narrator_settings
        .get(&target.identity)
        .cloned()
        .unwrap_or_default();
    let before = parameters.clone();
    if let Some([speed, pitch, pause, volume]) = values.voice {
        parameters.speed = crate::app::document::ControlValue::from_normalized(speed);
        parameters.pitch = crate::app::document::ControlValue::from_normalized(pitch);
        parameters.pause = crate::app::document::ControlValue::from_normalized(pause);
        parameters.volume = crate::app::document::ControlValue::from_normalized(volume);
    }
    for key in keys {
        if let Some(value) = values
            .emotions
            .get(&key)
            .copied()
            .or(zero_missing.then_some(0.0))
        {
            let value = crate::app::document::ControlValue::from_normalized(value);
            if parameters.emotions.get(&key).copied().unwrap_or_default() != value {
                app.portrait_state
                    .note_narrator_change(&target.identity, &key);
                parameters.emotions.insert(key, value);
            }
        }
    }
    if parameters != before {
        app.begin_edit();
        app.project
            .narrator_settings
            .insert(target.identity.clone(), parameters);
    }
}

pub(crate) fn isolate_focus(ctx: &egui::Context) {
    if let Some(state) = ctx.data(|d| d.get_temp::<State>(state_id())) {
        if state.notice.is_some() {
            crate::app::menus::native_alert::begin(ctx, Id::new("emotion-preset-notice"));
        } else if state.draft.is_some() {
            crate::app::menus::native_alert::begin(ctx, Id::new("emotion-preset-dialog"));
        }
    }
}

pub(crate) fn active(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<State>(state_id()))
        .is_some_and(|state| state.draft.is_some() || state.notice.is_some())
}

fn commit(state: &mut State, store: Store) -> Result<(), String> {
    if !state.writable {
        return Err("Emotion preset storage is unavailable; existing file was preserved".into());
    }
    store.save()?;
    state.store = Arc::new(store);
    Ok(())
}

fn draft(app: &EditorApp, state: &State, target: &Target, edit: Edit) -> Option<Draft> {
    capture_for(app, target, true)?;
    let matched = matched_for(app, state, target);
    let mut name = matched
        .as_ref()
        .filter(|g| !g.builtin)
        .map(|g| g.preset.name.clone())
        .unwrap_or_default();
    let mut include = if target.correction() {
        state.correction_include_voice
    } else {
        state.include_voice
    };
    if matches!(edit, Edit::New) {
        include = true;
        let base = crate::app::i18n::text(app.locale, "New Preset");
        name = base.to_owned();
        let groups = groups_for(app, state, target);
        let mut suffix = 2;
        while groups.iter().any(|g| g.preset.name == name) {
            name = format!("{base}{suffix}");
            suffix += 1;
        }
    }
    if let Edit::Rename(ref preset) | Edit::Delete(ref preset) = edit {
        name = preset.name.clone();
    }
    if matches!(edit, Edit::Update) && name.is_empty() {
        let presets = target.presets(&state.store);
        name = presets
            .iter()
            .find(|p| p.values.voice.is_some())
            .or_else(|| presets.first())?
            .name
            .clone();
    }
    Some(Draft {
        target: target.clone(),
        name,
        include,
        edit,
        focus: true,
    })
}

fn complete(app: &mut EditorApp, state: &mut State, draft: Draft) -> Result<(), String> {
    let current = capture_for(app, &draft.target, draft.include)
        .ok_or("The preset's source is no longer available")?;
    let all = groups_for(app, state, &draft.target);
    if matches!(draft.edit, Edit::New | Edit::Update) {
        if let Some(existing) = all.iter().find(|g| {
            g.preset.values.voice.is_some() == draft.include
                && matches(&g.preset.values, &current, false)
                && (!matches!(draft.edit, Edit::Update) || g.preset.name != draft.name)
        }) {
            return Err(format!(
                "{}{}",
                crate::app::i18n::text(app.locale, "Already saved as: "),
                existing.label
            ));
        }
    }
    if draft.name.is_empty() {
        return Err(crate::app::i18n::text(app.locale, "The name can't be empty.").into());
    }
    if draft.name.len() > 4096 {
        return Err("Preset name exceeds 4096 UTF-8 bytes".into());
    }
    let mut store = (*state.store).clone();
    let list = draft
        .target
        .map_mut(&mut store)
        .entry(draft.target.identity.clone())
        .or_default();
    match draft.edit {
        Edit::New => {
            if all.iter().any(|g| g.preset.name == draft.name) {
                return Err(crate::app::i18n::text(
                    app.locale,
                    "Duplicated name found in existing presets.",
                )
                .into());
            }
            list.push(Preset {
                name: draft.name,
                values: current,
            });
            commit(state, store)?;
        }
        Edit::Rename(old) => {
            if old.name == draft.name {
                return Ok(());
            }
            if all.iter().any(|g| g.preset.name == draft.name) {
                return Err(crate::app::i18n::text(
                    app.locale,
                    "Duplicated name found in existing presets.",
                )
                .into());
            }
            let preset = list
                .iter_mut()
                .find(|p| p.name == old.name)
                .ok_or("The preset is no longer available")?;
            preset.name = draft.name;
            commit(state, store)?;
        }
        Edit::Delete(old) => {
            let index = list
                .iter()
                .position(|p| p.name == old.name)
                .ok_or("The preset is no longer available")?;
            list.remove(index);
            commit(state, store)?;
        }
        Edit::Update => {
            let preset = list
                .iter_mut()
                .find(|p| p.name == draft.name)
                .ok_or("Preset name is not set.")?;
            let blocks = if draft.target.correction() {
                Vec::new()
            } else {
                app.project
                    .blocks
                    .iter()
                    .filter(|block| {
                        block.narrator.identity == draft.target.identity
                            && matches(&preset.values, &capture(app, block, true), true)
                    })
                    .map(|b| b.id)
                    .collect::<Vec<_>>()
            };
            preset.values = current.clone();
            commit(state, store)?;
            if !blocks.is_empty() {
                state.draft = Some(Draft {
                    edit: Edit::Others {
                        values: current,
                        blocks,
                    },
                    ..draft
                });
            }
        }
        Edit::Others { values, blocks } => {
            for id in blocks {
                if app
                    .project
                    .blocks
                    .iter()
                    .any(|b| b.id == id && b.narrator.identity == draft.target.identity)
                {
                    apply(app, id, &values, false);
                }
            }
        }
    }
    Ok(())
}

impl EditorApp {
    pub(crate) fn emotion_preset_actions(&mut self, ui: &egui::Ui) {
        ui::actions(self, ui);
    }
    pub(crate) fn emotion_preset_dialogs(&mut self, ctx: &egui::Context) {
        ui::dialogs(self, ctx);
    }
    pub(crate) fn emotion_preset_choices(&mut self, ui: &egui::Ui, anchor: Rect) {
        ui::choices(self, ui, anchor);
    }
}
