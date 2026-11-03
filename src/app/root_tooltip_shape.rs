use eframe::egui::{Pos2, Rect, Vec2};
use std::fmt::Write;

pub(super) fn svg(body: Rect, tip: Pos2) -> (Rect, Vec<u8>) {
    let r = 4.0;
    let mut points = vec![tip];
    let tl = body.min + Vec2::splat(r);
    let tr = Pos2::new(body.right() - r, body.top() + r);
    let br = body.max - Vec2::splat(r);
    let bl = Pos2::new(body.left() + r, body.bottom() - r);
    let quarter = std::f32::consts::FRAC_PI_2;
    let half = std::f32::consts::PI;
    let three = 3.0 * quarter;
    let full = std::f32::consts::TAU;
    if tip.x < body.left() {
        points.push(Pos2::new(body.left(), body.center().y - 8.0));
        arc(&mut points, tl, three, full);
        arc(&mut points, tr, 0.0, quarter);
        arc(&mut points, br, quarter, half);
        arc(&mut points, bl, half, three);
        points.push(Pos2::new(body.left(), body.center().y + 8.0));
    } else if tip.x > body.right() {
        points.push(Pos2::new(body.right(), body.center().y - 8.0));
        arc(&mut points, br, quarter, half);
        arc(&mut points, bl, half, three);
        arc(&mut points, tl, three, full);
        arc(&mut points, tr, 0.0, quarter);
        points.push(Pos2::new(body.right(), body.center().y + 8.0));
    } else {
        points.push(Pos2::new(body.center().x + 8.0, body.top()));
        arc(&mut points, tr, 0.0, quarter);
        points.push(Pos2::new(body.right(), body.bottom() - r));
        arc(&mut points, br, quarter, half);
        points.push(Pos2::new(body.right() - r, body.bottom()));
        arc(&mut points, bl, half, three);
        points.push(Pos2::new(body.left(), body.top() + r));
        arc(&mut points, tl, three, full);
        points.push(Pos2::new(body.center().x - 8.0, body.top()));
    }
    let bounds = points.iter().fold(Rect::NOTHING, |rect, &p| {
        rect.union(Rect::from_min_max(p, p))
    });
    let size = bounds.size().max(Vec2::splat(1.0));
    let bounds = Rect::from_min_size(bounds.min, size);
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}"><path fill="#2f2f33" fill-rule="nonzero" d=""##,
        size.x, size.y, size.x, size.y,
    );
    for (i, point) in points.iter().enumerate() {
        let point = *point - bounds.min;
        let _ = write!(
            svg,
            "{}{} {} ",
            if i == 0 { 'M' } else { 'L' },
            point.x,
            point.y
        );
    }
    svg.push_str("Z\"/></svg>");
    (bounds, svg.into_bytes())
}

fn arc(points: &mut Vec<Pos2>, center: Pos2, from: f32, to: f32) {
    let mut angle = from;
    while angle < to {
        points.push(center + Vec2::new(angle.sin() * 4.0, -(angle.cos() * 4.0)));
        angle += 0.05;
    }
    points.push(center + Vec2::new(to.sin() * 4.0, -(to.cos() * 4.0)));
}
