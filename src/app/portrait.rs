//! Standing portrait selection and layout.
#[path = "portrait_state.rs"]
mod state;
#[path = "portrait_view.rs"]
mod view;
use crate::app::catalog::Voice;
use eframe::egui::{self, Color32, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
pub use state::State;
use std::{
    io::Read,
    sync::{Arc, OnceLock},
};

pub const PANEL_WIDTH: f32 = 308.0;
pub const INSET: f32 = 24.0;

pub fn frame(panel: Rect) -> Rect {
    Rect::from_min_size(
        panel.min + Vec2::splat(INSET),
        (panel.size() - Vec2::splat(2.0 * INSET)).max(Vec2::ZERO),
    )
}

/// Resolve the selected expression for the current block and retained edit scope.
pub fn show(app: &mut crate::app::editor::EditorApp, ui: &mut Ui, panel: Rect) -> bool {
    let voice_and_block = app
        .selected()
        .and_then(|index| app.project.blocks.get(index))
        .and_then(|block| {
            app.catalog
                .identity_source(&block.narrator.identity)
                .zip(app.catalog.identity_metadata(&block.narrator.identity))
                .map(|(voice, metadata)| (voice, metadata, block))
        });
    let closed = if let Some((voice, metadata, block)) = voice_and_block {
        let emotion = app.portrait_state.selected_key(
            block,
            voice,
            metadata,
            app.project.narrator_settings.get(&block.narrator.identity),
        );
        let image = app.portrait_state.image(ui.ctx(), voice, &emotion);
        paint(
            ui,
            panel,
            Some(voice),
            image.as_ref(),
            &mut app.portrait_state.views,
            app.locale,
        )
    } else {
        paint(
            ui,
            panel,
            None,
            None,
            &mut app.portrait_state.views,
            app.locale,
        )
    };
    if let Some(problem) = app.portrait_state.views.take_problem() {
        app.error = Some(problem);
    }
    closed
}

/// Route portrait wheel events before framework smoothing and zoom gestures.
pub(crate) fn raw_input(ctx: &egui::Context, input: &mut egui::RawInput) {
    view::raw_input(ctx, input);
}
/// Missing expression artwork stays empty; do not substitute another picture.
fn paint(
    ui: &mut Ui,
    panel: Rect,
    voice: Option<&Voice>,
    image: Option<&state::Loaded>,
    views: &mut view::State,
    locale: crate::app::i18n::Locale,
) -> bool {
    let card = frame(panel);
    if card.width() <= 0.0 || card.height() <= 0.0 {
        return false;
    }
    ui.painter().rect_filled(
        card.translate(Vec2::splat(2.0)),
        4,
        Color32::from_black_alpha(20),
    );
    ui.painter().rect_filled(
        card,
        4,
        if image.is_some() {
            Color32::WHITE
        } else {
            Color32::from_gray(217)
        },
    );
    let mut image_ui = ui.new_child(egui::UiBuilder::new().max_rect(card));
    image_ui.set_clip_rect(card.intersect(ui.clip_rect()));
    let (image_rect, menu_close) = views.show(
        ui,
        panel,
        voice.map(|voice| voice.name.as_str()),
        image.map(|image| &image.portrait),
        locale,
    );
    if let Some((image, rect)) = image.zip(image_rect) {
        egui::Image::from_bytes(image.uri.clone(), image.portrait.bytes.clone())
            .paint_at(&image_ui, rect);
    } else if let Some(bytes) = placeholder_asset() {
        // Center the placeholder at its intrinsic size.
        egui::Image::from_bytes("bytes://reference-ui/image-008.svg", bytes).paint_at(
            &image_ui,
            Rect::from_center_size(card.center(), Vec2::new(40.0, 57.0)),
        );
    }
    ui.painter().rect_stroke(
        card,
        4,
        Stroke::new(1.0, Color32::from_gray(217)),
        StrokeKind::Inside,
    );
    let close = Rect::from_min_size(
        panel.min + Vec2::new(panel.width() - 64.0, 32.0),
        Vec2::splat(32.0),
    );
    let response = ui.interact(close, ui.id().with("close-portrait"), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Close")
    });
    if response.hovered() || response.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(close, 4, Color32::from_black_alpha(13));
    }
    if let (Some(voice), Some(bytes)) = (voice, close_asset()) {
        let [r, g, b] = voice.color;
        egui::Image::from_bytes("bytes://reference-ui/image-038.svg", bytes)
            .tint(Color32::from_rgba_unmultiplied(
                r,
                g,
                b,
                if ui.is_enabled() { 204 } else { 61 },
            ))
            .paint_at(ui, close);
    }
    menu_close || response.clicked()
}

fn close_asset() -> Option<Arc<[u8]>> {
    static BYTES: OnceLock<Option<Arc<[u8]>>> = OnceLock::new();
    BYTES
        .get_or_init(|| {
            let file =
                std::fs::File::open(crate::app::native_icons::directory()?.join("image-038.svg"))
                    .ok()?;
            let mut bytes = Vec::new();
            file.take(4097).read_to_end(&mut bytes).ok()?;
            (bytes.len() <= 4096).then(|| Arc::from(bytes))
        })
        .clone()
}

fn placeholder_asset() -> Option<Arc<[u8]>> {
    static BYTES: OnceLock<Option<Arc<[u8]>>> = OnceLock::new();
    BYTES
        .get_or_init(|| {
            let file =
                std::fs::File::open(crate::app::native_icons::directory()?.join("image-008.svg"))
                    .ok()?;
            let mut bytes = Vec::new();
            file.take(4097).read_to_end(&mut bytes).ok()?;
            (bytes.len() <= 4096).then(|| Arc::from(bytes))
        })
        .clone()
}
