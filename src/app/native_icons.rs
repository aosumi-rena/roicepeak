//! Editor control glyphs.
use crate::app::style::{self, Icon};
use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, Vec2};

pub fn paint_dialog(ui: &Ui, rect: Rect, icon: Icon) {
    paint(ui, rect, icon, style::TEXT, false, false);
}

pub fn paint(ui: &Ui, rect: Rect, icon: Icon, color: Color32, selected: bool, over: bool) {
    if !rect.is_positive() {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect);
    let color = if ui.is_enabled() {
        color
    } else {
        color.gamma_multiply(0.35)
    };
    let mode = matches!(icon, Icon::Accent | Icon::Intonation | Icon::Duration);
    if mode {
        let fill = match (selected, over && ui.is_enabled()) {
            (true, true) => Color32::from_gray(112),
            (true, false) => style::BAR,
            (false, true) => Color32::from_gray(232),
            (false, false) => style::BACKGROUND,
        };
        painter.rect(
            rect.shrink(1.0),
            4,
            fill,
            Stroke::new(1.0, style::BORDER),
            StrokeKind::Inside,
        );
    }
    let size = rect.width().min(rect.height());
    let scale = if matches!(icon, Icon::Person) {
        size
    } else {
        size.min(24.0)
    } / 24.0;
    let g = Glyph {
        painter,
        origin: rect.center() - Vec2::splat(12.0 * scale),
        scale,
        color,
    };
    match icon {
        Icon::Menu | Icon::PresetMore => {
            for y in [6.0, 12.0, 18.0] {
                g.dot([12.0, y], 1.5);
            }
        }
        Icon::Book => {
            g.outline([5.0, 4.0], [19.0, 20.0], 2.0);
            g.line(&[[8.0, 5.0], [8.0, 19.0]]);
            g.line(&[[11.0, 8.0], [16.0, 8.0]]);
            g.line(&[[11.0, 12.0], [16.0, 12.0]]);
        }
        Icon::Find => {
            g.circle([10.0, 10.0], 5.5);
            g.line(&[[14.0, 14.0], [19.0, 19.0]]);
        }
        Icon::Speed => {
            g.line(&[
                [4.0, 17.0],
                [4.6, 12.5],
                [7.5, 8.5],
                [12.0, 7.0],
                [16.5, 8.5],
                [19.4, 12.5],
                [20.0, 17.0],
            ]);
            g.line(&[[12.0, 16.0], [16.0, 11.0]]);
            g.dot([12.0, 16.0], 1.5);
        }
        Icon::Pitch => {
            g.line(&[[9.0, 17.0], [9.0, 6.0], [18.0, 4.0], [18.0, 15.0]]);
            g.dot([7.5, 17.5], 2.5);
            g.dot([16.5, 15.5], 2.5);
        }
        Icon::Pause => {
            g.clock([8.0, 9.0], 4.5);
            g.line(&[[12.0, 17.0], [19.0, 17.0]]);
            g.line(&[[15.0, 13.0], [19.0, 13.0]]);
        }
        Icon::Volume => {
            g.polygon(&[
                [4.0, 9.0],
                [7.0, 9.0],
                [12.0, 5.0],
                [12.0, 19.0],
                [7.0, 15.0],
                [4.0, 15.0],
            ]);
            g.line(&[[15.0, 9.0], [16.0, 10.5], [16.0, 13.5], [15.0, 15.0]]);
            g.line(&[
                [17.5, 6.5],
                [19.5, 9.0],
                [20.0, 12.0],
                [19.5, 15.0],
                [17.5, 17.5],
            ]);
        }
        Icon::Emotion => {
            g.circle([12.0, 12.0], 8.0);
            g.dot([9.0, 10.0], 1.0);
            g.dot([15.0, 10.0], 1.0);
            g.line(&[
                [8.5, 14.0],
                [10.0, 15.5],
                [12.0, 16.0],
                [14.0, 15.5],
                [15.5, 14.0],
            ]);
        }
        Icon::Previous | Icon::Next => {
            let mirror = |x| {
                if matches!(icon, Icon::Previous) {
                    24.0 - x
                } else {
                    x
                }
            };
            g.polygon(&[
                [mirror(6.0), 5.0],
                [mirror(16.0), 12.0],
                [mirror(6.0), 19.0],
            ]);
            g.line(&[[mirror(18.0), 5.0], [mirror(18.0), 19.0]]);
        }
        Icon::Play => g.polygon(&[[7.0, 4.5], [20.0, 12.0], [7.0, 19.5]]),
        Icon::Loop | Icon::BlockLoop => {
            g.line(&[[5.0, 11.0], [5.0, 9.0], [7.0, 7.0], [19.0, 7.0]]);
            g.line(&[[16.0, 4.0], [19.0, 7.0], [16.0, 10.0]]);
            g.line(&[[19.0, 13.0], [19.0, 15.0], [17.0, 17.0], [5.0, 17.0]]);
            g.line(&[[8.0, 14.0], [5.0, 17.0], [8.0, 20.0]]);
        }
        Icon::Undo | Icon::Redo => {
            let mirror = |x| {
                if matches!(icon, Icon::Redo) {
                    24.0 - x
                } else {
                    x
                }
            };
            g.line(&[[mirror(8.0), 5.0], [mirror(4.0), 9.0], [mirror(8.0), 13.0]]);
            g.line(&[
                [mirror(4.0), 9.0],
                [mirror(14.0), 9.0],
                [mirror(18.0), 11.0],
                [mirror(19.0), 14.0],
                [mirror(17.0), 17.0],
                [mirror(13.0), 19.0],
            ]);
        }
        Icon::Delete => {
            g.line(&[[5.0, 7.0], [19.0, 7.0]]);
            g.line(&[[9.0, 7.0], [9.0, 4.0], [15.0, 4.0], [15.0, 7.0]]);
            g.line(&[[7.0, 7.0], [8.0, 20.0], [16.0, 20.0], [17.0, 7.0]]);
            g.line(&[[12.0, 10.0], [12.0, 17.0]]);
        }
        Icon::Accent => {
            g.line(&[
                [5.0, 14.0],
                [10.0, 14.0],
                [10.0, 8.0],
                [17.0, 8.0],
                [17.0, 16.0],
                [20.0, 16.0],
            ]);
            g.dot([5.0, 14.0], 1.5);
            g.dot([20.0, 16.0], 1.5);
        }
        Icon::Intonation => {
            g.line(&[
                [5.0, 17.0],
                [7.0, 13.5],
                [10.0, 11.0],
                [14.0, 9.5],
                [19.0, 6.5],
            ]);
            g.dot([5.0, 17.0], 1.7);
            g.dot([19.0, 6.5], 1.7);
        }
        Icon::Duration => {
            g.outline([9.0, 5.0], [15.0, 19.0], 1.0);
            g.line(&[[6.0, 8.0], [3.0, 12.0], [6.0, 16.0]]);
            g.line(&[[18.0, 8.0], [21.0, 12.0], [18.0, 16.0]]);
        }
        Icon::Close | Icon::Clear => {
            if matches!(icon, Icon::Clear) {
                g.circle([12.0, 12.0], 8.0);
            }
            g.line(&[[8.5, 8.5], [15.5, 15.5]]);
            g.line(&[[15.5, 8.5], [8.5, 15.5]]);
        }
        Icon::Clock => g.clock([12.0, 12.0], 7.0),
        Icon::Up => g.line(&[[5.0, 15.0], [12.0, 8.5], [19.0, 15.0]]),
        Icon::Down => g.line(&[[5.0, 9.0], [12.0, 15.5], [19.0, 9.0]]),
        Icon::Person => {
            g.circle([12.0, 7.0], 4.0);
            g.line(&[
                [4.0, 21.0],
                [4.0, 18.0],
                [5.5, 15.5],
                [8.5, 14.0],
                [15.5, 14.0],
                [18.5, 15.5],
                [20.0, 18.0],
                [20.0, 21.0],
            ]);
        }
        Icon::Chevron => g.line(&[[7.0, 10.0], [12.0, 14.0], [17.0, 10.0]]),
    }
}

struct Glyph {
    painter: Painter,
    origin: Pos2,
    scale: f32,
    color: Color32,
}

impl Glyph {
    fn point(&self, point: [f32; 2]) -> Pos2 {
        self.origin + Vec2::from(point) * self.scale
    }
    fn stroke(&self) -> Stroke {
        Stroke::new(1.8 * self.scale, self.color)
    }
    fn line(&self, points: &[[f32; 2]]) {
        self.painter.add(Shape::line(
            points.iter().map(|&point| self.point(point)).collect(),
            self.stroke(),
        ));
    }
    fn polygon(&self, points: &[[f32; 2]]) {
        self.painter.add(Shape::convex_polygon(
            points.iter().map(|&point| self.point(point)).collect(),
            self.color,
            Stroke::NONE,
        ));
    }
    fn circle(&self, center: [f32; 2], radius: f32) {
        self.painter
            .circle_stroke(self.point(center), radius * self.scale, self.stroke());
    }
    fn dot(&self, center: [f32; 2], radius: f32) {
        self.painter
            .circle_filled(self.point(center), radius * self.scale, self.color);
    }
    fn outline(&self, min: [f32; 2], max: [f32; 2], radius: f32) {
        self.painter.rect_stroke(
            Rect::from_min_max(self.point(min), self.point(max)),
            (radius * self.scale).round() as u8,
            self.stroke(),
            StrokeKind::Inside,
        );
    }
    fn clock(&self, center: [f32; 2], radius: f32) {
        self.circle(center, radius);
        let [x, y] = center;
        self.line(&[
            [x, y - radius * 0.55],
            center,
            [x + radius * 0.4, y + radius * 0.25],
        ]);
    }
}
