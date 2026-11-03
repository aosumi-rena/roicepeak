use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

pub(crate) fn paint(ui: &egui::Ui, slot: egui::layers::ShapeIdx, outer: Rect) {
    paint_direction(ui, slot, outer, true);
}

pub(crate) fn paint_direction(
    ui: &egui::Ui,
    slot: egui::layers::ShapeIdx,
    outer: Rect,
    below: bool,
) {
    let body = outer.shrink(9.0);
    let tip = Pos2::new(body.center().x, outer.top() + 1.0);
    let mut points = vec![tip, Pos2::new(tip.x + 8.0, body.top())];
    for (center, quarter) in [
        (Pos2::new(body.right() - 4.0, body.top() + 4.0), 0),
        (Pos2::new(body.right() - 4.0, body.bottom() - 4.0), 1),
        (Pos2::new(body.left() + 4.0, body.bottom() - 4.0), 2),
        (Pos2::new(body.left() + 4.0, body.top() + 4.0), 3),
    ] {
        for step in 0..=16 {
            let angle = (quarter as f32 + step as f32 / 16.0) * std::f32::consts::FRAC_PI_2;
            points.push(center + Vec2::new(angle.sin() * 4.0, -angle.cos() * 4.0));
        }
    }
    points.push(Pos2::new(tip.x - 8.0, body.top()));
    if !below {
        for point in &mut points {
            point.y = outer.top() + outer.bottom() - point.y;
        }
        points.reverse();
    }
    let mut fill = egui::Mesh::default();
    fill.colored_vertex(body.center(), Color32::WHITE);
    for point in &points {
        fill.colored_vertex(*point, Color32::WHITE);
    }
    for index in 0..points.len() as u32 {
        fill.add_triangle(0, index + 1, (index + 1) % points.len() as u32 + 1);
    }
    let scale = ui.ctx().pixels_per_point();
    let width = scale.round_ties_even() / scale;
    let shadow = egui::epaint::Shadow {
        offset: [3, 3],
        blur: 3,
        spread: 0,
        color: Color32::from_black_alpha(15),
    };
    ui.painter().set(
        slot,
        egui::Shape::Vec(vec![
            shadow.as_shape(body, 4).into(),
            egui::Shape::mesh(fill),
            egui::Shape::closed_line(
                points,
                egui::Stroke::new(width, Color32::from_black_alpha(31)),
            ),
        ]),
    );
}
