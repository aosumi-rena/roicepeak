//! Block narrator tiles and inspector image menus.
#[path = "narrator_menu.rs"]
pub(crate) mod menu;
use crate::app::{editor::EditorApp, style};
use eframe::egui::{self, Align2, Color32, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum PickerKind {
    Block,
    Inspector,
}

#[derive(Clone, Copy)]
struct Anchor {
    rect: Rect,
    kind: PickerKind,
    initial: bool,
    block: Option<u64>,
}

pub(crate) fn remember_anchor(ctx: &egui::Context, rect: Rect, kind: PickerKind) {
    ctx.data_mut(|data| {
        data.insert_temp(
            Id::new("narrator-picker-anchor"),
            Anchor {
                rect,
                kind,
                initial: true,
                block: None,
            },
        )
    });
}

pub(crate) fn remember_block_anchor(ctx: &egui::Context, rect: Rect, block: u64) {
    ctx.data_mut(|data| {
        data.insert_temp(
            Id::new("narrator-picker-anchor"),
            Anchor {
                rect,
                kind: PickerKind::Block,
                initial: true,
                block: Some(block),
            },
        )
    });
}

impl EditorApp {
    pub(crate) fn narrator_picker(&mut self, ctx: &egui::Context, screen: Rect) {
        let Some(anchor) =
            ctx.data(|data| data.get_temp::<Anchor>(Id::new("narrator-picker-anchor")))
        else {
            self.popup = None;
            return;
        };
        let ordered = match crate::app::narrator_order::identity_indices(&self.catalog) {
            Ok(indices) => indices,
            Err(error) => {
                self.error = Some(error);
                self.popup = None;
                return;
            }
        };
        if anchor.kind == PickerKind::Inspector {
            let choices = ordered
                .iter()
                .map(|&index| self.catalog.voices[index].key.clone())
                .collect::<Vec<_>>();
            let shown = egui::Area::new(Id::new("inspector-narrator-menu-parent"))
                .order(egui::Order::Foreground)
                .fixed_pos(anchor.rect.min)
                .show(ctx, |ui| {
                    menu::show_list_at(
                        self,
                        ui,
                        Id::new("inspector-narrator-menu"),
                        anchor.rect,
                        anchor.initial,
                        &choices,
                    )
                });
            ctx.data_mut(|data| {
                data.insert_temp(
                    Id::new("narrator-picker-anchor"),
                    Anchor {
                        initial: false,
                        ..anchor
                    },
                )
            });
            if let Some(key) = shown.inner.key {
                self.choose_narrator(&key, anchor.block);
            } else if shown.inner.closed {
                self.popup = None;
            }
            return;
        }
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.popup = None;
            return;
        }
        let groups: [Vec<usize>; 2] = std::array::from_fn(|group| {
            ordered
                .iter()
                .copied()
                .filter(|&index| self.catalog.voices[index].can_commercial == (group == 1))
                .collect()
        });
        let columns = groups.iter().map(Vec::len).max().unwrap_or(0).clamp(1, 4);
        let width = {
            let headings = [
                self.tr("キャラクター", "Character Voices"),
                self.tr("商用可能ナレーター", "Available for Commercial Usage"),
            ];
            let heading_width = groups
                .iter()
                .zip(headings)
                .filter(|(group, _)| !group.is_empty())
                .map(|(_, title)| {
                    ctx.fonts_mut(|fonts| {
                        fonts
                            .layout_no_wrap(
                                title.to_owned(),
                                style::font(20.88 / 1.48),
                                style::TEXT,
                            )
                            .size()
                            .x
                    })
                })
                .fold(0.0_f32, f32::max);
            (columns as f32 * 132.0 + 56.0).max(heading_width + 56.0)
        }
        .min(screen.width());
        let group_height = |count: usize| {
            if count == 0 {
                0.0
            } else {
                45.0 + count.div_ceil(4) as f32 * 116.0 - 8.0
            }
        };
        let content_height = {
            group_height(groups[0].len())
                + group_height(groups[1].len())
                + if groups.iter().all(|group| !group.is_empty()) {
                    26.0
                } else {
                    0.0
                }
                + 36.0
        }
        .max(64.0);
        let height = content_height.min(screen.height());
        // Anchor the block menu at the button's upper-right corner.
        let preferred = anchor.rect.right_top();
        let position = Pos2::new(
            preferred
                .x
                .clamp(screen.left(), (screen.right() - width).max(screen.left())),
            preferred
                .y
                .clamp(screen.top(), (screen.bottom() - height).max(screen.top())),
        );
        let mut chosen = None;
        let area = egui::Area::new(Id::new("native-narrator-selector"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .show(ctx, |ui| {
                ui.set_min_size(Vec2::new(width, height));
                ui.set_max_size(Vec2::new(width, height));
                let outer = Rect::from_min_size(ui.min_rect().min, Vec2::new(width, height));
                ui.painter().rect_filled(outer, 6, Color32::WHITE);
                ui.painter().rect_stroke(
                    outer.shrink(0.5),
                    6,
                    Stroke::new(1.0, style::BORDER),
                    StrokeKind::Inside,
                );
                let inset = 8.0;
                ui.scope_builder(egui::UiBuilder::new().max_rect(outer.shrink(inset)), |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("narrator-selector-scroll")
                        .max_height((height - inset * 2.0).max(0.0))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = Vec2::ZERO;
                            ui.set_min_width((width - inset * 2.0).max(0.0));
                            if self.catalog.voices.is_empty() {
                                let rect = Rect::from_min_size(
                                    ui.cursor().min,
                                    Vec2::new((width - inset * 2.0).max(0.0), 32.0),
                                );
                                if style::text_button(
                                    ui,
                                    rect,
                                    self.tr("ボイスを設定...", "Configure voices..."),
                                    true,
                                    true,
                                )
                                .clicked()
                                {
                                    self.open_resource_settings();
                                    self.popup = None;
                                }
                            } else {
                                let start = ui.cursor().min;
                                let mut y = start.y;
                                for (group_index, group) in groups.iter().enumerate() {
                                    if group.is_empty() {
                                        continue;
                                    }
                                    if group_index == 1 && !groups[0].is_empty() {
                                        let gap = 26.0;
                                        let line_y = y + 20.0;
                                        ui.painter().line_segment(
                                            [
                                                Pos2::new(start.x, line_y),
                                                Pos2::new(start.x + width - inset * 2.0, line_y),
                                            ],
                                            Stroke::new(1.0, style::BORDER),
                                        );
                                        y += gap;
                                    }
                                    {
                                        let title = if group_index == 0 {
                                            self.tr("キャラクター", "Character Voices")
                                        } else {
                                            self.tr(
                                                "商用可能ナレーター",
                                                "Available for Commercial Usage",
                                            )
                                        };
                                        ui.painter().text(
                                            Pos2::new(start.x + 20.0, y + 22.5),
                                            Align2::LEFT_CENTER,
                                            title,
                                            style::font(20.88 / 1.48),
                                            style::TEXT,
                                        );
                                        y += 45.0;
                                    }
                                    for (position, index) in group.iter().copied().enumerate() {
                                        let voice = &self.catalog.voices[index];
                                        let display_name = self.instance_name(&voice.key);
                                        let rect = Rect::from_min_size(
                                            Pos2::new(
                                                start.x + 20.0 + (position % 4) as f32 * 132.0,
                                                y + (position / 4) as f32 * 116.0,
                                            ),
                                            Vec2::new(124.0, 108.0),
                                        );
                                        let response = ui.interact(
                                            rect,
                                            ui.id().with(("narrator", &voice.key)),
                                            Sense::click(),
                                        );
                                        response.widget_info(|| {
                                            egui::WidgetInfo::labeled(
                                                egui::WidgetType::Button,
                                                true,
                                                &display_name,
                                            )
                                        });
                                        let selected = anchor
                                            .block
                                            .and_then(|id| {
                                                self.project.blocks.iter().find(|b| b.id == id)
                                            })
                                            .map(|block| &block.narrator)
                                            .is_some_and(|current| current.identity == voice.name);
                                        let alpha = if response.is_pointer_button_down_on() {
                                            26
                                        } else if response.hovered() || selected {
                                            13
                                        } else {
                                            0
                                        };
                                        if alpha != 0 {
                                            ui.painter().rect_filled(
                                                rect,
                                                6,
                                                Color32::from_black_alpha(alpha),
                                            );
                                        }
                                        let avatar = Rect::from_min_size(
                                            rect.min + Vec2::new(32.0, 12.0),
                                            Vec2::splat(60.0),
                                        );
                                        self.paint_instance_avatar(ui, &voice.key, avatar);
                                        let label = Rect::from_min_size(
                                            rect.min + Vec2::new(8.0, 76.0),
                                            Vec2::new(108.0, 24.0),
                                        );
                                        // Use the shared font calibration.
                                        let galley = ui.painter().layout(
                                            display_name,
                                            style::font(23.04 / 1.48),
                                            self.instance_color(&voice.key),
                                            label.width(),
                                        );
                                        let painter = ui
                                            .painter()
                                            .with_clip_rect(label.intersect(ui.clip_rect()));
                                        let text_position = label.center() - galley.size() * 0.5;
                                        painter.galley(
                                            text_position,
                                            galley,
                                            self.instance_color(&voice.key),
                                        );
                                        if anchor.initial && selected {
                                            ui.scroll_to_rect(rect, None);
                                        }
                                        if response.clicked() {
                                            chosen = Some(voice.key.clone());
                                        }
                                    }
                                    y += group.len().div_ceil(4) as f32 * 116.0 - 8.0;
                                }
                                let bottom = y + 20.0;
                                ui.allocate_rect(
                                    Rect::from_min_max(
                                        start,
                                        Pos2::new(start.x + width - inset * 2.0, bottom),
                                    ),
                                    Sense::hover(),
                                );
                            }
                        });
                });
            });
        ctx.data_mut(|data| {
            data.insert_temp(
                Id::new("narrator-picker-anchor"),
                Anchor {
                    initial: false,
                    ..anchor
                },
            )
        });
        if let Some(key) = chosen {
            self.choose_narrator(&key, anchor.block);
        } else if ctx.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|position| !area.response.rect.contains(position))
        }) {
            self.popup = None;
        }
    }
    fn choose_narrator(&mut self, key: &str, clicked: Option<u64>) {
        let resolved = self
            .voice_target_indices(clicked)
            .into_iter()
            .map(|index| {
                crate::app::narrator_order::resolve_choice(
                    &self.catalog,
                    &self.project.blocks[index].narrator,
                    key,
                )
                .map(|reference| (index, reference))
            })
            .collect::<Result<Vec<_>, _>>();
        match resolved {
            Ok(resolved) => {
                if resolved
                    .iter()
                    .any(|(index, reference)| self.project.blocks[*index].narrator != *reference)
                {
                    self.begin_edit();
                    for (index, reference) in resolved {
                        let block = &mut self.project.blocks[index];
                        if block.narrator != reference {
                            block.narrator = reference;
                            block.segments.clear();
                        }
                    }
                }
            }
            Err(error) => self.error = Some(error),
        }
        self.popup = None;
    }
}
