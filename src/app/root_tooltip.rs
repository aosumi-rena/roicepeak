use eframe::egui::{self, Color32, Id, Pos2, Rect, Response, Vec2};
use std::sync::Arc;

#[path = "root_tooltip_shape.rs"]
mod shape;

#[derive(Clone)]
struct Request {
    frame: u64,
    anchor: Rect,
    text: String,
}

#[derive(Clone)]
struct Image {
    bytes: Arc<[u8]>,
    uri: String,
}

pub(crate) struct Scope {
    ctx: egui::Context,
    previous: bool,
}

impl Scope {
    pub(crate) fn new(ctx: &egui::Context) -> Self {
        let key = Id::new("root-tooltip-main-scope");
        let previous = ctx.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
        ctx.data_mut(|d| d.insert_temp(key, true));
        Self {
            ctx: ctx.clone(),
            previous,
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.ctx
            .data_mut(|d| d.insert_temp(Id::new("root-tooltip-main-scope"), self.previous));
    }
}

pub(crate) fn attach(response: Response, text: &str) -> Response {
    let main_content = response
        .ctx
        .data(|d| d.get_temp::<bool>(Id::new("root-tooltip-main-scope")))
        .unwrap_or(false);
    if response.ctx.viewport_id() != egui::ViewportId::ROOT || !main_content {
        return response.on_hover_text(text);
    }
    // Hover scheduling remains with egui until the reference timer is qualified.
    if !text.is_empty()
        && response.enabled()
        && egui::Tooltip::should_show_tooltip(&response, false)
    {
        let request = Request {
            frame: response.ctx.cumulative_frame_nr(),
            anchor: response.rect,
            text: text.to_owned(),
        };
        response
            .ctx
            .data_mut(|d| d.insert_temp(Id::new("root-tooltip-request"), request));
    }
    response
}

pub(crate) fn paint(ui: &mut egui::Ui, root: Rect, enabled: bool) {
    let ctx = ui.ctx().clone();
    let request_key = Id::new("root-tooltip-request");
    let request = ctx.data(|d| d.get_temp::<Request>(request_key));
    ctx.data_mut(|d| d.remove::<Request>(request_key));
    let Some(request) = request else { return };
    if !enabled
        || ctx.viewport_id() != egui::ViewportId::ROOT
        || request.frame != ctx.cumulative_frame_nr()
        || !root.is_positive()
    {
        return;
    }
    let color = Color32::from_rgba_unmultiplied(255, 255, 255, 224);
    let mut job = egui::text::LayoutJob::simple(
        request.text,
        crate::app::style::font(20.16 / 1.48),
        color,
        400.0,
    );
    job.halign = egui::Align::Center;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let desired = Vec2::new(
        (galley.size().x + 40.0).trunc(),
        (galley.size().y + 16.0).trunc(),
    );
    let anchor = request.anchor.translate(-root.min.to_vec2());
    let (position, tip) = if anchor.left() < 100.0 {
        (
            Pos2::new(
                anchor.right() + 8.0,
                anchor.center().y - (desired.y / 2.0).trunc(),
            ),
            Pos2::new(anchor.right(), anchor.center().y),
        )
    } else if anchor.right() > root.width() - 100.0 {
        (
            Pos2::new(
                anchor.left() - 8.0 - desired.x,
                anchor.center().y - (desired.y / 2.0).trunc(),
            ),
            Pos2::new(anchor.left(), anchor.center().y),
        )
    } else {
        (
            Pos2::new(
                anchor.center().x - (desired.x / 2.0).trunc(),
                anchor.bottom() + 8.0,
            ),
            Pos2::new(anchor.center().x, anchor.bottom()),
        )
    };
    let maximum = (root.size() - desired).max(Vec2::ZERO);
    let position = Pos2::new(
        position.x.clamp(0.0, maximum.x),
        position.y.clamp(0.0, maximum.y),
    );
    let body = Rect::from_min_size(position + root.min.to_vec2(), desired.min(root.size()));
    let (bounds, bytes) = shape::svg(body, tip + root.min.to_vec2());
    let image_key = Id::new("root-tooltip-image");
    let old = ctx.data(|d| d.get_temp::<Image>(image_key));
    let image = if let Some(old) = old.filter(|old| old.bytes.as_ref() == bytes.as_slice()) {
        old
    } else {
        if let Some(old) = ctx.data(|d| d.get_temp::<Image>(image_key)) {
            ctx.forget_image(&old.uri);
        }
        let uri = format!(
            "bytes://roicepeak-root-tooltip-{:016x}.svg",
            Id::new(&bytes).value()
        );
        let image = Image {
            bytes: bytes.into(),
            uri,
        };
        ctx.data_mut(|d| d.insert_temp(image_key, image.clone()));
        image
    };
    let layer = egui::LayerId::new(egui::Order::Tooltip, Id::new("root-tooltip-paint"));
    let mut tooltip = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(root));
    tooltip.set_clip_rect(root);
    egui::Image::from_bytes(image.uri, image.bytes).paint_at(&tooltip, bounds);
    tooltip
        .painter()
        .galley(body.center() - galley.size() * 0.5, galley, color);
}
