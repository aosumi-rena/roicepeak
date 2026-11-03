#[path = "replacement_menu.rs"]
mod replacement_menu;
use crate::app::style::text_button as button;
use crate::app::{catalog::Catalog, document::Project, editor::EditorApp, style};
use eframe::egui::{self, Color32, Rect, Sense, Vec2};
use std::collections::BTreeSet;

struct Choice {
    identity: String,
    original: String,
    target: String,
}

pub(crate) struct State {
    rows: Vec<Choice>,
    scroll_started: bool,
}

impl State {
    pub(crate) fn new(project: &Project, catalog: &Catalog) -> Result<Self, String> {
        if catalog.voices.is_empty() {
            return Err("No voice resources are configured".into());
        }
        let mut seen = BTreeSet::new();
        let mut rows = Vec::new();
        for block in &project.blocks {
            if seen.insert(block.narrator.identity.clone()) {
                let original = catalog
                    .voice(&block.narrator)
                    .or_else(|| {
                        catalog
                            .voices
                            .iter()
                            .find(|voice| voice.name == block.narrator.identity)
                    })
                    .map(|voice| voice.key.clone())
                    .unwrap_or_default();
                rows.push(Choice {
                    identity: block.narrator.identity.clone(),
                    original: original.clone(),
                    target: original,
                });
            }
        }
        Ok(Self {
            rows,
            scroll_started: false,
        })
    }

    fn apply(&self, project: &mut Project, catalog: &Catalog) -> Result<(), String> {
        let mut changes = Vec::new();
        for (index, block) in project.blocks.iter().enumerate() {
            let row = self
                .rows
                .iter()
                .find(|row| row.identity == block.narrator.identity)
                .ok_or("Project narrators changed while replacement was open")?;
            let key = if row.target == row.original {
                block.narrator.clone()
            } else {
                crate::app::narrator_order::resolve_choice(catalog, &block.narrator, &row.target)?
            };
            changes.push((index, key));
        }
        for (index, key) in changes {
            let block = &mut project.blocks[index];
            if block.narrator != key {
                block.narrator = key;
                block.segments.clear();
            }
        }
        Ok(())
    }
}

impl EditorApp {
    pub(crate) fn replace_voices_window(&mut self, ctx: &egui::Context) {
        let Some(mut state) = self.voice_replacement.take() else {
            return;
        };
        let choices = match crate::app::narrator_order::identity_indices(&self.catalog) {
            Ok(indices) => indices
                .into_iter()
                .map(|index| self.catalog.voices[index].key.clone())
                .collect::<Vec<_>>(),
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        let list_height = (state.rows.len() as f32 * 84.0 - 8.0).clamp(0.0, 160.0);
        let height = list_height + 185.0;
        let mut apply = false;
        let mut cancel = !egui::Popup::is_any_open(ctx)
            && ctx.input(|input| input.key_pressed(egui::Key::Escape));
        egui::Modal::new(egui::Id::new("replace-voices"))
            .backdrop_color(Color32::TRANSPARENT)
            .frame(crate::app::dialog_controls::alert_frame())
            .show(ctx, |ui| {
                let (_, outer) = ui.allocate_space(Vec2::new(595.0, height));
                let at =
                    |x, y, w, h| Rect::from_min_size(outer.min + Vec2::new(x, y), Vec2::new(w, h));
                style::label(
                    ui,
                    at(24.0, 24.0, 547.0, 24.0).left_center(),
                    self.tr("ボイスの置換", "Replace Voices"),
                    23.04 / 1.48,
                    Color32::from_rgb(71, 78, 132),
                );
                style::label(
                    ui,
                    at(24.0, 68.0, 274.0, 25.0).left_center(),
                    self.tr("現在のボイス", "Current Voice"),
                    20.88 / 1.48,
                    style::TEXT,
                );
                style::label(
                    ui,
                    at(298.0, 68.0, 273.0, 25.0).left_center(),
                    self.tr("置換", "Replacement"),
                    20.88 / 1.48,
                    style::TEXT,
                );
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(at(24.0, 101.0, 547.0, list_height)),
                    |ui| {
                        let viewport = at(24.0, 101.0, 547.0, list_height);
                        let extent = (state.rows.len() as f32 * 84.0 - 8.0).max(0.0);
                        let reset = !state.scroll_started;
                        state.scroll_started = true;
                        crate::app::inspector_scroll::show_content(
                            ui,
                            viewport,
                            Vec2::new(547.0, extent),
                            Vec2::ZERO,
                            reset,
                            |ui, content| {
                                let start = content.min;
                                for (index, row) in state.rows.iter_mut().enumerate() {
                                    let rect = Rect::from_min_size(
                                        start + Vec2::new(0.0, index as f32 * 84.0),
                                        Vec2::new(547.0, 76.0),
                                    );
                                    ui.painter().rect_filled(rect, 4, Color32::from_gray(240));
                                    self.voice_replacement_label(
                                        ui,
                                        Rect::from_min_size(
                                            rect.min + Vec2::new(12.0, 8.0),
                                            Vec2::splat(60.0),
                                        ),
                                        Rect::from_min_size(
                                            rect.min + Vec2::new(84.0, 8.0),
                                            Vec2::new(96.0, 60.0),
                                        ),
                                        &row.original,
                                        0.4,
                                    );
                                    ui.painter().text(
                                        Rect::from_min_size(
                                            rect.min + Vec2::new(212.0, 8.0),
                                            Vec2::new(62.0, rect.height() - 16.0),
                                        )
                                        .center(),
                                        egui::Align2::CENTER_CENTER,
                                        "→",
                                        style::font(23.04 / 1.48),
                                        Color32::from_gray(102),
                                    );
                                    let target = Rect::from_min_size(
                                        rect.min + Vec2::new(274.0, 8.0),
                                        Vec2::new(261.0, 60.0),
                                    );
                                    let clip = ui.clip_rect();
                                    if extent > list_height {
                                        ui.set_clip_rect(clip.intersect(Rect::from_min_max(
                                            viewport.min,
                                            egui::pos2(viewport.right() - 16.0, viewport.bottom()),
                                        )));
                                    }
                                    let response = ui.interact(
                                        target,
                                        ui.id().with(("replacement-target", index)),
                                        Sense::click(),
                                    );
                                    ui.set_clip_rect(clip);
                                    let alpha = if response.hovered() {
                                        13
                                    } else if response.is_pointer_button_down_on() {
                                        26
                                    } else {
                                        0
                                    };
                                    if alpha != 0 {
                                        ui.painter().rect_filled(
                                            target,
                                            4,
                                            Color32::from_black_alpha(alpha),
                                        );
                                    }
                                    self.voice_replacement_label(
                                        ui,
                                        Rect::from_min_size(target.min, Vec2::splat(60.0)),
                                        Rect::from_min_size(
                                            target.min + Vec2::new(75.0, 2.0),
                                            Vec2::new(157.0, 56.0),
                                        ),
                                        &row.target,
                                        0.7,
                                    );
                                    caret(ui, target.min + Vec2::new(241.0, 29.0));
                                    self.replacement_choices(ui, &response, index, row, &choices);
                                }
                            },
                        );
                    },
                );
                if button(
                    ui,
                    at(363.0, height - 56.0, 100.0, 32.0),
                    self.tr("置換", "Replace"),
                    true,
                    true,
                )
                .clicked()
                {
                    apply = true;
                }
                if button(
                    ui,
                    at(471.0, height - 56.0, 100.0, 32.0),
                    self.tr("キャンセル", "Cancel"),
                    false,
                    true,
                )
                .clicked()
                {
                    cancel = true;
                }
            });
        if apply {
            if let Err(error) = state.apply(&mut self.project, &self.catalog) {
                self.error = Some(error);
            }
        } else if !cancel {
            self.voice_replacement = Some(state);
        }
    }

    fn voice_replacement_label(
        &mut self,
        ui: &egui::Ui,
        avatar: Rect,
        label: Rect,
        key: &str,
        min_scale: f32,
    ) {
        self.paint_instance_avatar(ui, key, avatar);
        fitted_name(
            ui,
            label,
            &self.instance_name(key),
            self.instance_color(key),
            min_scale,
        );
    }

    fn replacement_choices(
        &mut self,
        ui: &egui::Ui,
        anchor: &egui::Response,
        index: usize,
        row: &mut Choice,
        choices: &[String],
    ) {
        replacement_menu::show(self, ui, anchor, index, row, choices);
    }
}

fn caret(ui: &egui::Ui, origin: egui::Pos2) {
    let mut mesh = egui::Mesh::default();
    for (x, y) in [
        (1.0, 0.0),
        (6.0, 4.0),
        (11.0, 0.0),
        (12.0, 1.0),
        (6.5, 6.0),
        (5.5, 6.0),
        (0.0, 1.0),
    ] {
        mesh.colored_vertex(origin + Vec2::new(x, y), Color32::from_gray(179));
    }
    for [a, b, c] in [[1, 2, 3], [1, 3, 4], [1, 4, 5], [1, 5, 6], [1, 6, 0]] {
        mesh.add_triangle(a, b, c);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

fn fitted_name(ui: &egui::Ui, rect: Rect, name: &str, color: Color32, min_scale: f32) {
    let mut galley = ui
        .painter()
        .layout_no_wrap(name.to_owned(), style::font(23.04 / 1.48), color);
    let scale = (rect.width() / galley.size().x.max(1.0)).clamp(min_scale, 1.0);
    if scale < 1.0 {
        let galley = std::sync::Arc::make_mut(&mut galley);
        for row in &mut galley.rows {
            row.pos.x *= scale;
            let row = std::sync::Arc::make_mut(&mut row.row);
            for vertex in &mut row.visuals.mesh.vertices {
                vertex.pos.x *= scale;
            }
            row.visuals.mesh_bounds.min.x *= scale;
            row.visuals.mesh_bounds.max.x *= scale;
        }
        galley.rect.min.x *= scale;
        galley.rect.max.x *= scale;
        galley.mesh_bounds.min.x *= scale;
        galley.mesh_bounds.max.x *= scale;
    }
    let at = egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5);
    ui.painter()
        .with_clip_rect(rect.intersect(ui.clip_rect()))
        .galley(at, galley, color);
}
