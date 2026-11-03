use crate::app::{
    catalog::Catalog,
    dialog_controls::{self, ComboRow},
    document::{VoiceRef, VoiceVersion},
    i18n::{self, Locale},
    native_menu::{self, Item},
};
use eframe::egui::{self, Color32, Rect, Sense};

pub(super) fn show(
    ui: &egui::Ui,
    rect: Rect,
    reference: &VoiceRef,
    catalog: &Catalog,
    locale: Locale,
    block: Option<u64>,
    color: Color32,
    enabled: bool,
    uniform: Option<VoiceVersion>,
) -> Option<VoiceVersion> {
    let versions = catalog.versions(reference);
    let latest = i18n::tr(locale, "最新", "Latest");
    let missing = i18n::tr(locale, "(未インストール)", "(Not Installed)");
    let caption = if !enabled || reference.is_empty() || uniform.is_none() {
        String::new()
    } else {
        match uniform.unwrap() {
            VoiceVersion::Latest => latest.to_owned(),
            VoiceVersion::Exact(version) if versions.contains(&version) => format!("v{version}"),
            VoiceVersion::Exact(version) => format!("v{version} {missing}"),
        }
    };
    let available = enabled && !reference.is_empty() && ui.is_enabled();
    dialog_controls::combo_face_colored(
        ui,
        rect,
        &caption,
        if available {
            color
        } else {
            Color32::from_gray(179)
        },
    );
    let id = ui.id().with(("narrator-version", block));
    let open_id = id.with("open");
    let response = ui.interact(rect, id, Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, available, &caption));
    let mut open = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(open_id).unwrap_or(false));
    let input = dialog_controls::combo_input(ui, &response, open, available, || {
        let mut rows = vec![
            ComboRow {
                id: Some(1),
                enabled: true,
            },
            ComboRow {
                id: None,
                enabled: false,
            },
        ];
        rows.extend(versions.iter().map(|v| ComboRow {
            id: Some(u64::from(*v) + 10),
            enabled: true,
        }));
        let current = uniform.map(|v| match v {
            VoiceVersion::Latest => 1,
            VoiceVersion::Exact(v) => u64::from(v) + 10,
        });
        (current, rows)
    });
    let mut selected = input.selected.map(|id| {
        if id == 1 {
            VoiceVersion::Latest
        } else {
            VoiceVersion::Exact((id - 10) as u32)
        }
    });
    if input.toggle {
        open = !open;
        if !open {
            native_menu::close(ui.ctx(), id.with("menu"));
        }
    }
    if !available {
        open = false;
        native_menu::close(ui.ctx(), id.with("menu"));
    }
    if open {
        let effective = selected.or(uniform);
        let labels: Vec<_> = versions
            .iter()
            .map(|version| format!("v{version}"))
            .collect();
        let mut items = vec![
            Item::CheckedAction {
                text: latest,
                shortcut: String::new(),
                enabled: true,
                checked: effective == Some(VoiceVersion::Latest),
                action: VoiceVersion::Latest,
            },
            Item::Separator,
        ];
        for (version, label) in versions.iter().zip(&labels) {
            items.push(Item::CheckedAction {
                text: label,
                shortcut: String::new(),
                enabled: true,
                checked: effective == Some(VoiceVersion::Exact(*version)),
                action: VoiceVersion::Exact(*version),
            });
        }
        let current = items
            .iter()
            .rposition(|item| matches!(item, Item::CheckedAction { checked: true, .. }));
        let outcome = native_menu::show_selector_at_width(
            ui,
            id.with("menu"),
            rect,
            &items,
            rect.width(),
            current,
        );
        if outcome.closed {
            open = false;
        }
        selected = outcome.action.or(selected);
    }
    ui.ctx().data_mut(|data| data.insert_temp(open_id, open));
    selected.filter(|version| Some(*version) != uniform)
}
