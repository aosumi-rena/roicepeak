use super::super::Entry;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};

const LOW: i32 = 0x2000;
const HIGH: i32 = 0x2001;
pub(super) const CELL: f32 = 48.0;
const ACTIVE: Color32 = Color32::from_rgb(40, 131, 235);
const GUIDE: Color32 = Color32::from_gray(212);

#[derive(Default)]
pub(super) struct State {
    basis: Option<(String, usize, Vec<i32>)>,
    codes: Vec<i32>,
    dragging: bool,
    blocked: bool,
}

impl State {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    fn sync(&mut self, entry: &Entry, moras: usize) -> Result<(), String> {
        let basis = (
            entry.reading.clone(),
            entry.accent,
            entry.native.overwrite_accents.clone(),
        );
        if self.basis.as_ref() == Some(&basis) {
            return Ok(());
        }
        if moras > 4096 || entry.native.overwrite_accents.len() > moras + 1 {
            self.basis = Some(basis);
            self.codes.clear();
            self.dragging = false;
            self.blocked = true;
            return Err("Dictionary accent overrides exceed the reading bounds".into());
        }
        self.blocked = false;
        self.codes = (0..=moras)
            .map(|i| {
                let high = match entry.accent {
                    0 => i != 0,
                    1 => i == 0,
                    n => i != 0 && i < n,
                };
                if high { HIGH } else { LOW }
            })
            .collect();
        self.codes[..entry.native.overwrite_accents.len()]
            .copy_from_slice(&entry.native.overwrite_accents);
        self.basis = Some(basis);
        self.dragging = false;
        Ok(())
    }

    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        entry: &mut Entry,
        moras: usize,
    ) -> Result<bool, String> {
        if !ui.is_enabled() && self.dragging {
            self.dragging = false;
            self.basis = None;
        }
        self.sync(entry, moras)?;
        if !ui.is_enabled() {
            if moras != 0 && !self.blocked {
                paint(ui, rect, &self.codes);
            }
            return Ok(false);
        }
        if moras == 0 || self.blocked {
            return Ok(false);
        }
        let response = ui.interact(
            rect,
            ui.id().with("dictionary-accent"),
            Sense::click_and_drag(),
        );
        let (pressed, down, released, position) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.latest_pos(),
            )
        });
        if pressed && response.is_pointer_button_down_on() {
            self.dragging = true;
        }
        if self.dragging && down {
            if let Some(p) = position.filter(|p| p.x.is_finite() && p.y.is_finite()) {
                let left = rect.width() * 0.5 - self.codes.len() as f32 * CELL * 0.5;
                let x = (p.x - rect.left()) as i32;
                let y = (p.y - rect.top()) as i32;
                let at = ((x as f32 - left) / CELL).floor();
                if at >= 0.0 && (at as usize) < self.codes.len() {
                    self.codes[at as usize] = if y < rect.height() as i32 / 2 {
                        HIGH
                    } else {
                        LOW
                    };
                    ui.ctx().request_repaint();
                }
            }
        }
        let mut committed = false;
        if self.dragging && released {
            entry.accent = if self.codes.last() == Some(&HIGH) {
                0
            } else {
                self.codes[..moras]
                    .iter()
                    .rposition(|c| *c == HIGH)
                    .map_or(1, |i| i + 1)
            };
            entry.native.overwrite_accents = self.codes[..moras].to_vec();
            self.basis = Some((
                entry.reading.clone(),
                entry.accent,
                entry.native.overwrite_accents.clone(),
            ));
            self.dragging = false;
            committed = true;
        } else if self.dragging && !down {
            self.dragging = false;
            self.basis = None;
        }
        paint(ui, rect, &self.codes);
        Ok(committed)
    }
}

fn point(rect: Rect, codes: &[i32], i: usize) -> Pos2 {
    let left = rect.width() * 0.5 - codes.len() as f32 * CELL * 0.5;
    Pos2::new(
        rect.left() + left + (i as f32 + 0.5) * CELL,
        rect.top()
            + if codes[i] == HIGH {
                11.0
            } else {
                (rect.height() - 16.0).max(0.0) + 5.0
            },
    )
}

fn paint(ui: &egui::Ui, rect: Rect, codes: &[i32]) {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let high = rect.top() + 11.0;
    let low = rect.top() + (rect.height() - 16.0).max(0.0) + 5.0;
    let area = Color32::from_rgba_unmultiplied(40, 131, 235, 13);
    for i in 0..codes.len() {
        let p = point(rect, codes, i);
        if p.x < painter.clip_rect().left() - 3.0 || p.x > painter.clip_rect().right() + 3.0 {
            continue;
        }
        let span = (rect.height() - 16.0).max(0.0) + 8.0 - 3.0 - 3.0 - 2.0 - 1.0 - 17.0;
        let count = (span / 4.333_330_2).floor().max(-1.0) as i32 + 1;
        let first = 17.0 + (span - (count - 1) as f32 * 4.333_330_2) * 0.5;
        for j in 0..count {
            painter.circle_filled(
                Pos2::new(p.x, rect.top() + first + j as f32 * 4.333_330_2),
                1.0,
                GUIDE,
            );
        }
        let other_y = if codes[i] == HIGH { low } else { high };
        painter.circle_stroke(Pos2::new(p.x, other_y), 3.0, Stroke::new(1.0, GUIDE));
    }
    for i in 1..codes.len() {
        let a = point(rect, codes, i - 1);
        let b = point(rect, codes, i);
        if b.x < painter.clip_rect().left() || a.x > painter.clip_rect().right() {
            continue;
        }
        let controls = [a, a + Vec2::new(24.0, 0.0), b - Vec2::new(24.0, 0.0), b];
        let mut mesh = egui::Mesh::default();
        for j in 0..=16 {
            let t = j as f32 / 16.0;
            let u = 1.0 - t;
            let p = controls[0].to_vec2() * (u * u * u)
                + controls[1].to_vec2() * (3.0 * u * u * t)
                + controls[2].to_vec2() * (3.0 * u * t * t)
                + controls[3].to_vec2() * (t * t * t);
            mesh.colored_vertex(p.to_pos2(), area);
            mesh.colored_vertex(Pos2::new(p.x, low), area);
            if j > 0 {
                let at = (j * 2) as u32;
                mesh.add_triangle(at - 2, at - 1, at);
                mesh.add_triangle(at, at - 1, at + 1);
            }
        }
        painter.add(Shape::mesh(mesh));
        painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
            controls,
            false,
            Color32::TRANSPARENT,
            Stroke::new(1.0, ACTIVE),
        ));
    }
    for i in 0..codes.len() {
        let p = point(rect, codes, i);
        if painter.clip_rect().expand(3.0).contains(p) {
            painter.circle_filled(p, 3.0, ACTIVE);
        }
    }
}
