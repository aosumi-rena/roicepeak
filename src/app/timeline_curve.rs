use eframe::egui::{self, Color32, Pos2, Stroke};

pub(super) fn accent(ui: &egui::Ui, points: &[Pos2], bottom: f32, color: Color32) {
    let Some(first) = points.first() else { return };
    let mut fill = Vec::with_capacity(points.len() * 16 + 3);
    fill.push(*first);
    for pair in points.windows(2) {
        let [a, b] = [pair[0], pair[1]];
        for step in 1..=16 {
            let t = step as f32 / 16.0;
            let u = 1.0 - t;
            fill.push(egui::pos2(
                u * u * u * a.x
                    + 3.0 * u * u * t * (a.x + 28.0)
                    + 3.0 * u * t * t * (b.x - 28.0)
                    + t * t * t * b.x,
                u * u * u * a.y + 3.0 * u * u * t * a.y + 3.0 * u * t * t * b.y + t * t * t * b.y,
            ));
        }
    }
    let mut mesh = egui::Mesh::default();
    let tint = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 13);
    for point in &fill {
        mesh.colored_vertex(*point, tint);
        mesh.colored_vertex(egui::pos2(point.x, bottom), tint);
    }
    for index in 0..fill.len().saturating_sub(1) as u32 {
        let base = index * 2;
        mesh.add_triangle(base, base + 2, base + 1);
        mesh.add_triangle(base + 1, base + 2, base + 3);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    for pair in points.windows(2) {
        ui.painter()
            .add(egui::epaint::CubicBezierShape::from_points_stroke(
                [
                    pair[0],
                    pair[0] + egui::vec2(28.0, 0.0),
                    pair[1] - egui::vec2(28.0, 0.0),
                    pair[1],
                ],
                false,
                Color32::TRANSPARENT,
                Stroke::new(
                    1.0,
                    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 153),
                ),
            ));
    }
    for point in points {
        ui.painter().circle_filled(*point, 3.0, color);
    }
}
