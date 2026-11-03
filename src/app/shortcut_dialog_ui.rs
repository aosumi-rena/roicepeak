use super::*;
use crate::app::{native_menu::Item, shortcuts::Slot, style};
use eframe::egui::{Color32, Rect, Sense, Stroke, StrokeKind, Vec2};

const FONT: f32 = 23.04 / 1.48;
fn text(locale: crate::app::i18n::Locale, value: &str) -> &str {
    crate::app::i18n::text(locale, value)
}

pub(super) fn show(state: &mut State, app: &mut EditorApp, ctx: &egui::Context, first: bool) {
    let locale = app.locale;
    let modal = state.confirmation.is_some() || state.notice.is_some();
    let mut close = false;
    let mut key_rect = None;
    egui::Modal::new(id())
        .backdrop_color(Color32::TRANSPARENT)
        .frame(crate::app::dialog_controls::floating_frame())
        .show(ctx, |ui| {
            let (_, rect) = ui.allocate_space(Vec2::new(516.0, 640.0));
            let at = |x, y, w, h| Rect::from_min_size(rect.min + Vec2::new(x, y), Vec2::new(w, h));
            let background = ui.interact(
                rect,
                id().with("keyboard"),
                Sense::focusable_noninteractive(),
            );
            if first {
                background.request_focus();
            }
            if modal {
                ui.disable();
            }
            ui.painter()
                .with_clip_rect(at(24.0, 25.0, 420.0, 32.0))
                .text(
                    rect.min + Vec2::new(24.0, 41.0),
                    egui::Align2::LEFT_CENTER,
                    text(locale, "Keyboard Shortcuts"),
                    style::font(FONT),
                    crate::app::dialog_controls::HEADING,
                );
            let more = at(460.0, 25.0, 32.0, 32.0);
            if style::icon(
                ui,
                "shortcut-menu",
                more,
                style::Icon::PresetMore,
                style::TEXT,
                text(locale, "Keyboard Shortcuts"),
            )
            .clicked()
            {
                state.commit(&mut app.shortcuts);
                state.header = if state.header.is_some() {
                    None
                } else {
                    Some(more)
                };
            }
            let search_rect = at(24.0, 73.0, 468.0, 40.0);
            let search = ui.put(
                search_rect,
                egui::TextEdit::singleline(&mut state.search)
                    .id(id().with("search"))
                    .font(style::font(FONT))
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin {
                        left: 16,
                        right: 44,
                        top: 0,
                        bottom: 0,
                    })
                    .text_color(Color32::from_gray(102))
                    .vertical_align(egui::Align::Center),
            );
            ui.painter().rect_stroke(
                search_rect,
                4,
                Stroke::new(
                    if search.has_focus() { 2.0 } else { 1.0 },
                    Color32::from_gray(if search.has_focus() { 125 } else { 221 }),
                ),
                StrokeKind::Inside,
            );
            let mut changed = search.changed();
            if state.search.is_empty() {
                crate::app::native_icons::paint_dialog(
                    ui,
                    Rect::from_center_size(
                        search_rect.right_center() - Vec2::new(24.0, 0.0),
                        Vec2::splat(16.0),
                    ),
                    48,
                );
            } else {
                let clear =
                    Rect::from_min_size(search_rect.min + Vec2::new(436.0, 4.0), Vec2::splat(32.0));
                let response = ui.interact(clear, id().with("clear-search"), Sense::click());
                if response.hovered() || response.is_pointer_button_down_on() {
                    ui.painter()
                        .rect_filled(clear, 4, Color32::from_black_alpha(20));
                }
                crate::app::native_icons::paint_dialog(ui, clear, 49);
                if response.clicked() {
                    state.search.clear();
                    changed = true;
                }
            }
            let sections = [Slot::MAIN_MENU, Slot::CONTROL, Slot::TEXT_EDITOR];
            let captions = ["Main Menu", "Control", "Text Editor"];
            let visible: Vec<Vec<Slot>> = sections
                .iter()
                .map(|slots| {
                    slots
                        .iter()
                        .copied()
                        .filter(|slot| text(locale, slot.label()).contains(&state.search))
                        .collect()
                })
                .collect();
            let height: f32 = (0..3)
                .map(|index| {
                    44.0 + if state.expanded[index] {
                        visible[index].len() as f32 * 44.0
                    } else {
                        0.0
                    }
                })
                .sum();
            let viewport = at(24.0, 137.0, 488.0, 428.0);
            ui.push_id("shortcut-list", |ui| {
                crate::app::inspector_scroll::show_content(
                    ui,
                    viewport,
                    Vec2::new(470.0, height),
                    Vec2::ZERO,
                    changed,
                    |ui, content| {
                        let mut y = content.top();
                        for section in 0..3 {
                            let header = Rect::from_min_size(
                                egui::pos2(content.left(), y),
                                Vec2::new(470.0, 44.0),
                            );
                            let response = ui.interact(
                                header,
                                id().with(("section", section)),
                                Sense::click(),
                            );
                            ui.painter().text(
                                header.min + Vec2::new(16.0, 16.0),
                                egui::Align2::LEFT_CENTER,
                                text(locale, captions[section]),
                                style::font(FONT),
                                Color32::from_gray(102),
                            );
                            let caption = text(locale, captions[section]);
                            let caption_width = ui
                                .painter()
                                .layout_no_wrap(caption.to_owned(), style::font(FONT), style::TEXT)
                                .size()
                                .x;
                            let line_start = header.left() + 16.0 + caption_width + 12.0;
                            if line_start < header.right() {
                                ui.painter().line_segment(
                                    [
                                        egui::pos2(line_start, header.top() + 16.0),
                                        egui::pos2(header.right(), header.top() + 16.0),
                                    ],
                                    Stroke::new(1.0, Color32::from_gray(225)),
                                );
                            }
                            group_caret(
                                ui,
                                header.min + Vec2::new(0.0, 14.0),
                                state.expanded[section],
                            );
                            if response.clicked() {
                                state.expanded[section] = !state.expanded[section];
                            }
                            if state.expanded[section] {
                                for (index, &slot) in visible[section].iter().enumerate() {
                                    let row = Rect::from_min_size(
                                        egui::pos2(content.left(), y + 38.0 + index as f32 * 44.0),
                                        Vec2::new(470.0, 44.0),
                                    );
                                    if row.intersects(viewport) {
                                        row_ui(
                                            state,
                                            &mut app.shortcuts,
                                            ui,
                                            row,
                                            slot,
                                            locale,
                                            &mut key_rect,
                                        );
                                    }
                                }
                            }
                            y += 44.0
                                + if state.expanded[section] {
                                    visible[section].len() as f32 * 44.0
                                } else {
                                    0.0
                                };
                        }
                    },
                );
            });
            if style::text_button(
                ui,
                at(392.0, 584.0, 100.0, 32.0),
                text(locale, "Close"),
                true,
                true,
            )
            .clicked()
            {
                close = true;
            }
            if let Some(anchor) = state.header {
                let labels = [
                    "Save Shortcut Set",
                    "Load Shortcut Set",
                    "Reset All Shortcuts",
                ]
                .map(|key| text(locale, key).to_owned());
                let items = [
                    Item::Action {
                        text: &labels[0],
                        shortcut: String::new(),
                        enabled: true,
                        action: 0,
                    },
                    Item::Action {
                        text: &labels[1],
                        shortcut: String::new(),
                        enabled: true,
                        action: 1,
                    },
                    Item::Separator,
                    Item::Action {
                        text: &labels[2],
                        shortcut: String::new(),
                        enabled: true,
                        action: 2,
                    },
                ];
                let outcome = native_menu::show_at(ui, id().with("menu"), anchor, &items);
                if outcome.closed {
                    state.header = None;
                }
                match outcome.action {
                    Some(0) => {
                        if let Err(error) = super::save(&app.shortcuts, locale) {
                            state.notice = Some(error);
                        }
                    }
                    Some(1) => state.confirmation = Some(Confirmation::Overwrite(Operation::Load)),
                    Some(2) => state.confirmation = Some(Confirmation::Overwrite(Operation::Reset)),
                    _ => (),
                }
            }
        });
    state.field_rect = key_rect;
    let conflicts = state
        .draft
        .as_ref()
        .map(|draft| app.shortcuts.conflicts(draft.slot, draft.chord))
        .unwrap_or_default();
    let conflict_open = !conflicts.is_empty() && state.field_rect.is_some() && !modal;
    let mut clicked_conflict = false;
    if conflict_open {
        clicked_conflict = conflict(state, app, ctx, &conflicts);
    }
    if let Some(slot) = state.refocus.take() {
        ctx.memory_mut(|memory| memory.request_focus(field_id(slot)));
    }
    if state.confirmation.is_none() && state.header.is_none() && !clicked_conflict {
        if let Some(draft) = &state.draft {
            if !ctx.memory(|memory| memory.has_focus(field_id(draft.slot))) {
                state.commit(&mut app.shortcuts);
            }
        }
    }
    if close {
        app.shortcut_settings = false;
    }
}

fn row_ui(
    state: &mut State,
    map: &mut ShortcutMap,
    ui: &mut egui::Ui,
    row: Rect,
    slot: Slot,
    locale: crate::app::i18n::Locale,
    key_rect: &mut Option<Rect>,
) {
    let label_rect = Rect::from_min_size(row.min + Vec2::new(0.0, 6.0), Vec2::new(290.0, 32.0));
    ui.painter()
        .with_clip_rect(label_rect.intersect(ui.clip_rect()))
        .text(
            label_rect.left_center(),
            egui::Align2::LEFT_CENTER,
            text(locale, slot.label()),
            style::font(FONT),
            Color32::from_gray(102),
        );
    let field = Rect::from_min_size(row.min + Vec2::new(326.0, 4.0), Vec2::new(144.0, 36.0));
    let response = ui.interact(field, field_id(slot), Sense::click());
    if response.clicked() || response.gained_focus() {
        state.select(map, slot);
        response.request_focus();
    }
    let draft = state.draft.as_ref().filter(|draft| draft.slot == slot);
    let chord = draft
        .map(|draft| draft.chord)
        .unwrap_or_else(|| map.chord(slot));
    let conflict = !map.conflicts(slot, chord).is_empty();
    let focused = response.has_focus();
    if draft.is_some() {
        *key_rect = Some(field);
    }
    ui.painter().rect_stroke(
        field.shrink(2.0),
        4,
        Stroke::new(
            if focused || conflict { 2.0 } else { 1.0 },
            if conflict {
                Color32::from_rgb(242, 110, 110)
            } else if focused {
                Color32::from_rgba_unmultiplied(27, 43, 121, 204)
            } else {
                Color32::from_gray(221)
            },
        ),
        StrokeKind::Inside,
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(field.shrink(3.0), 4, Color32::from_black_alpha(5));
    }
    let label = chord.map(Chord::label).unwrap_or_default();
    ui.painter()
        .with_clip_rect(field.intersect(ui.clip_rect()))
        .text(
            field.center(),
            egui::Align2::CENTER_CENTER,
            &label,
            style::font(FONT),
            Color32::from_gray(102),
        );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{}: {}", text(locale, slot.label()), label),
        )
    });
    if response.hovered() {
        state.reset_hover = Some(slot);
    }
    if ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|pos| label_rect.contains(pos))
    {
        state.reset_hover = None;
    }
    if state.reset_hover == Some(slot) {
        let reset = Rect::from_min_size(row.min + Vec2::new(290.0, 6.0), Vec2::splat(32.0));
        let reset_response = ui.interact(reset, id().with(("reset", slot)), Sense::click());
        if reset_response.hovered() || reset_response.is_pointer_button_down_on() {
            ui.painter()
                .rect_filled(reset, 4, Color32::from_black_alpha(20));
        }
        crate::app::native_icons::paint_dialog(ui, reset, 16);
        if reset_response.clicked() {
            state.select(map, slot);
            state.default_draft(slot);
        }
    }
}

fn conflict(
    state: &mut State,
    app: &mut EditorApp,
    ctx: &egui::Context,
    conflicts: &[Slot],
) -> bool {
    let Some(anchor) = state.field_rect else {
        return false;
    };
    let labels = conflicts
        .iter()
        .map(|slot| text(app.locale, slot.label()))
        .collect::<Vec<_>>()
        .join("\n");
    let message = format!(
        "{}\n{}",
        text(app.locale, "The shortcut is already used for:"),
        labels
    );
    let measured = ctx.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(message.clone(), style::font(FONT), style::TEXT)
            .size()
            .x
            .ceil()
    });
    let width = (measured.max(260.0) + 28.0).min(ctx.viewport_rect().width());
    let height = 134.0;
    let screen = ctx.viewport_rect();
    let mut pos = egui::pos2(anchor.center().x - width * 0.5, anchor.bottom());
    if pos.y + height > screen.bottom() {
        pos.y = anchor.top() - height;
    }
    pos.x = pos
        .x
        .clamp(screen.left(), (screen.right() - width).max(screen.left()));
    pos.y = pos
        .y
        .clamp(screen.top(), (screen.bottom() - height).max(screen.top()));
    let mut clicked = false;
    egui::Area::new(id().with("conflict"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
            style::card_radius(ui, rect, 4);
            ui.painter().with_clip_rect(rect.shrink(14.0)).text(
                rect.min + Vec2::splat(14.0),
                egui::Align2::LEFT_TOP,
                &message,
                style::font(FONT),
                style::TEXT,
            );
            let at = |x| {
                Rect::from_min_size(
                    rect.min + Vec2::new(x, height - 38.0),
                    Vec2::new(112.0, 24.0),
                )
            };
            if style::text_button(
                ui,
                at(width - 252.0),
                text(app.locale, "Cancel"),
                false,
                true,
            )
            .clicked()
            {
                state.cancel_draft(&app.shortcuts);
                clicked = true;
            }
            if style::text_button(
                ui,
                at(width - 126.0),
                text(app.locale, "Use Default"),
                false,
                true,
            )
            .clicked()
            {
                if let Some(draft) = &state.draft {
                    state.confirmation = Some(Confirmation::Default(draft.slot));
                }
                clicked = true;
            }
        });
    clicked
}

fn group_caret(ui: &egui::Ui, origin: egui::Pos2, expanded: bool) {
    let points = [
        [1.0, 0.0],
        [4.0, 3.0],
        [7.0, 0.0],
        [8.0, 1.0],
        [4.5, 5.0],
        [3.5, 5.0],
        [0.0, 1.0],
    ];
    let mut mesh = egui::Mesh::default();
    let angle = -1.570_845_f32;
    let (sin, cos) = angle.sin_cos();
    for [mut x, mut y] in points {
        if !expanded {
            let dx = x - 4.0;
            let dy = y - 2.5;
            x = cos * dx - sin * dy + 4.0;
            y = sin * dx + cos * dy + 2.5;
        }
        mesh.colored_vertex(origin + Vec2::new(x, y), Color32::from_gray(102));
    }
    mesh.indices
        .extend_from_slice(&[6, 0, 1, 1, 2, 3, 1, 3, 4, 6, 1, 4, 4, 5, 6]);
    ui.painter().add(egui::Shape::mesh(mesh));
}
