//! Script block geometry and font metrics.
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::sync::Arc;

pub const DEFAULT_EM: f32 = 23.04 / 1.48;
const LINE_SPACING: f32 = f32::from_bits(0x3f9f_ffff);

pub fn font_height(em: f32) -> f32 {
    em * 1.48
}

pub fn layout(ui: &egui::Ui, text: &str, width: f32, em: f32, color: Color32) -> Arc<egui::Galley> {
    let height = font_height(em);
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        super::super::style::font(em),
        color,
        width.max(1.0),
    );
    job.keep_trailing_whitespace = true;
    job.round_output_to_gui = false;
    for section in &mut job.sections {
        section.format.line_height = Some(height);
        section.format.valign = egui::Align::TOP;
    }
    let mut galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let output = Arc::make_mut(&mut galley);
    let advance = height * LINE_SPACING;
    output.mesh_bounds = Rect::NOTHING;
    for (index, row) in output.rows.iter_mut().enumerate() {
        row.pos.y = index as f32 * advance;
        output.mesh_bounds = output
            .mesh_bounds
            .union(row.visuals.mesh_bounds.translate(row.pos.to_vec2()));
    }
    output.rect.max.y = height + output.rows.len().saturating_sub(1) as f32 * advance;
    galley
}

pub struct Geometry {
    pub card: Rect,
    pub editor: Rect,
    pub narrator: Rect,
    pub grip: Rect,
    pub timestamp: Pos2,
    pub delete: Rect,
    pub loop_button: Rect,
    pub stride: f32,
}

impl Geometry {
    pub fn new(ui: &egui::Ui, card_min: Pos2, card_width: f32, text: &str, em: f32) -> Self {
        let outer_width = (card_width + 20.0).max(0.0);
        let available = (outer_width - 36.0).max(0.0);
        let stacked = outer_width <= 800.0;
        let reserved = available.min(if stacked { 47.0 } else { 268.0 });
        let gap = if stacked {
            0.0
        } else {
            (available - reserved).min(48.0)
        };
        let editor_width = (available - reserved - gap).max(1.0);
        let galley = layout(ui, text, (editor_width - 2.0).max(1.0), em, Color32::BLACK);
        // Include row padding and enforce the minimum editor height.
        let editor_height = (galley.rect.height().round() + 6.0).max(26.0);
        let mut stride = editor_height + if stacked { 93.0 } else { 54.0 };
        let outer_min = card_min - Vec2::splat(10.0);
        let mut delete = Rect::from_min_size(
            outer_min + Vec2::new(outer_width - if stacked { 50.0 } else { 58.0 }, 26.0),
            Vec2::splat(32.0),
        );
        let mut loop_button = delete.translate(Vec2::new(-40.0, 0.0));
        if !stacked {
            let text_right = outer_min.x + 18.0 + reserved + galley.rect.width().round_ties_even();
            let reflow = stride > 88.0 || text_right > loop_button.left();
            if reflow {
                if stride <= 88.0 {
                    stride += 24.0;
                }
                let overlap = ((116.0 - stride).max(0.0) * 0.5).trunc();
                delete = Rect::from_min_size(
                    outer_min + Vec2::new(outer_width - 58.0, 26.0 - overlap),
                    Vec2::splat(32.0),
                );
                loop_button = Rect::from_min_size(
                    outer_min + Vec2::new(outer_width - 106.0, stride - 58.0 + overlap),
                    Vec2::splat(32.0),
                );
            }
        }
        let card = Rect::from_min_size(card_min, Vec2::new(card_width.max(1.0), stride - 20.0));
        let editor = Rect::from_min_size(
            outer_min + Vec2::new(18.0 + reserved, if stacked { 66.0 } else { 27.0 }),
            Vec2::new(editor_width, editor_height),
        );
        let narrator = Rect::from_min_size(
            card_min
                + if stacked {
                    Vec2::new(129.0, 16.0)
                } else {
                    Vec2::new(126.0, 14.0)
                },
            Vec2::new(154.0, 32.0),
        );
        let grip = Rect::from_min_size(
            card_min
                + if stacked {
                    Vec2::new(9.0, 15.0)
                } else {
                    Vec2::new(8.0, 14.0)
                },
            Vec2::splat(32.0),
        );
        let timestamp = card_min
            + if stacked {
                Vec2::new(50.0, 32.0)
            } else {
                Vec2::new(40.0, 31.0)
            };
        Self {
            card,
            editor,
            narrator,
            grip,
            timestamp,
            delete,
            loop_button,
            stride,
        }
    }
}
