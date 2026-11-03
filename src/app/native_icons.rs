//! Renderer for locally supplied icon assets.
use crate::app::style::Icon;
use eframe::egui::{self, Color32, Pos2, Rect, Ui, Vec2};
use std::{
    collections::BTreeMap,
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

struct Asset {
    number: u8,
    gray_white: bool,
    viewport: Vec2,
    component: Rect,
    drawable: Rect,
}

impl Asset {
    fn new(number: u8, viewport: [f32; 2], component: [f32; 4], drawable: [f32; 4]) -> Self {
        let rect = |v: [f32; 4]| Rect::from_min_size(Pos2::new(v[0], v[1]), Vec2::new(v[2], v[3]));
        Self {
            number,
            gray_white: false,
            viewport: viewport.into(),
            component: rect(component),
            drawable: rect(drawable),
        }
    }
}

type IconCache = BTreeMap<(u8, bool), Option<Arc<[u8]>>>;
static CACHE: OnceLock<Mutex<IconCache>> = OnceLock::new();

pub fn install(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

pub fn directory() -> Option<PathBuf> {
    Some(
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("resources")
            .join("ui"),
    )
}

fn bytes(number: u8, gray_white: bool) -> Option<Arc<[u8]>> {
    let mut cache = CACHE.get_or_init(Default::default).lock().ok()?;
    cache
        .entry((number, gray_white))
        .or_insert_with(|| {
            let path = directory()?.join(format!("image-{number:03}.svg"));
            let file = std::fs::File::open(path).ok()?;
            let mut data = Vec::new();
            file.take(256 * 1024 + 1).read_to_end(&mut data).ok()?;
            if data.len() > 256 * 1024 {
                return None;
            }
            if gray_white {
                if !matches!(number, 16 | 49) {
                    return None;
                }
                let svg = std::str::from_utf8(&data).ok()?;
                let white = "fill=\"white\"";
                if svg.matches(white).count() != 1 {
                    return None;
                }
                Some(Arc::from(
                    svg.replacen(white, "fill=\"#666666\"", 1).into_bytes(),
                ))
            } else if matches!(number, 12 | 53) {
                let svg = std::str::from_utf8(&data).ok()?;
                let black = "fill=\"black\"";
                if svg.matches(black).count() != 1 {
                    return None;
                }
                Some(Arc::from(
                    svg.replacen(black, "fill=\"#666666\"", 1).into_bytes(),
                ))
            } else {
                Some(Arc::from(data))
            }
        })
        .clone()
}

fn toolbar_asset(kind: Icon) -> Option<Asset> {
    let full = |number| {
        Asset::new(
            number,
            [32.0, 32.0],
            [0.0, 0.0, 32.0, 32.0],
            [0.0, 0.0, 32.0, 32.0],
        )
    };
    Some(match kind {
        Icon::Menu => Asset::new(
            52,
            [19.0, 18.0],
            [8.0, 3.0, 4.0, 13.0],
            [8.39648, 3.0, 3.00002, 13.0],
        ),
        Icon::Book => Asset::new(
            5,
            [19.0, 18.0],
            [2.0, 1.0, 15.0, 17.0],
            [2.89648, 1.5, 14.00002, 16.0],
        ),
        Icon::Find => Asset::new(
            41,
            [24.0, 24.0],
            [3.0, 4.0, 17.0, 17.0],
            [3.0, 4.0, 16.0938, 16.125],
        ),
        Icon::Volume => Asset::new(
            18,
            [33.0, 32.0],
            [7.0, 8.0, 18.0, 16.0],
            [7.09277, 8.9375, 17.00003, 14.1563],
        ),
        Icon::Loop => Asset::new(
            33,
            [24.0, 24.0],
            [2.0, 2.0, 20.0, 21.0],
            [2.5, 2.16992, 19.0, 20.41018],
        ),
        Icon::Speed => full(47),
        Icon::Pitch => full(1),
        Icon::Pause => full(15),
        Icon::Emotion => full(6),
        Icon::Previous => full(24),
        Icon::Play => full(10),
        Icon::Next => full(45),
        Icon::Undo => full(16),
        Icon::Redo => full(31),
        Icon::Delete => Asset::new(
            9,
            [14.0, 17.0],
            [0.0, 0.0, 14.0, 17.0],
            [0.0, 0.5, 14.0, 16.0],
        ),
        Icon::BlockLoop => Asset::new(
            23,
            [24.0, 24.0],
            [4.0, 3.0, 16.0, 18.0],
            [4.0, 3.5078125, 16.0, 16.984362],
        ),
        Icon::PresetMore => Asset::new(
            53,
            [24.0, 24.0],
            [10.0, 5.0, 4.0, 13.0],
            [10.5, 5.0, 3.0, 13.0],
        ),
        // Unsupported widget states have no icon mapping.
        Icon::Chevron | Icon::Accent | Icon::Intonation | Icon::Duration => return None,
    })
}

fn mode_asset(kind: Icon, selected: bool, over: bool) -> Option<Asset> {
    let choices = match kind {
        Icon::Accent => [2, 20, 43, 28],
        Icon::Intonation => [13, 11, 22, 34],
        Icon::Duration => [27, 4, 7, 44],
        _ => return None,
    };
    let state = usize::from(selected) * 2 + usize::from(over);
    let (viewport, bounds) = match state {
        0 => ([36.0, 36.0], [0.0, 0.0, 37.0, 37.0]),
        1 | 2 => ([66.0, 66.0], [15.0, 13.0, 36.0, 36.0]),
        _ => ([36.0, 36.0], [0.0, 0.0, 36.0, 36.0]),
    };
    Some(Asset::new(choices[state], viewport, bounds, bounds))
}

/// Returns false when the requested asset or geometry is unavailable.
pub fn paint(ui: &Ui, rect: Rect, kind: Icon, selected: bool, over: bool) -> bool {
    let mode = mode_asset(kind, selected, over);
    let is_mode = mode.is_some();
    let Some(asset) = mode.or_else(|| toolbar_asset(kind)) else {
        return false;
    };
    paint_asset(ui, rect, asset, is_mode)
}

pub fn paint_dialog(ui: &Ui, rect: Rect, number: u8) -> bool {
    let mut asset = match number {
        12 => Asset::new(
            12,
            [12.0, 13.0],
            [0.0, 0.0, 12.0, 13.0],
            [0.0, 0.0, 12.0, 13.0],
        ),
        35 => Asset::new(
            35,
            [14.0, 14.0],
            [0.0, 0.0, 14.0, 14.0],
            [0.0, 0.0, 14.0, 14.0],
        ),
        16 => Asset::new(
            16,
            [32.0, 32.0],
            [0.0, 0.0, 32.0, 32.0],
            [0.0, 0.0, 32.0, 32.0],
        ),
        48 => Asset::new(
            48,
            [17.0, 17.0],
            [0.0, 0.0, 17.0, 17.0],
            [0.0, 0.0, 17.0, 17.0],
        ),
        49 => Asset::new(
            49,
            [24.0, 24.0],
            [0.0, 0.0, 24.0, 24.0],
            [0.0, 0.0, 24.0, 24.0],
        ),
        14 => Asset::new(
            14,
            [24.0, 24.0],
            [0.0, 0.0, 24.0, 24.0],
            [0.0, 0.0, 24.0, 24.0],
        ),
        21 => Asset::new(
            21,
            [24.0, 24.0],
            [5.0, 7.0, 14.0, 9.0],
            [5.234375, 7.7578125, 13.5078, 7.5078373],
        ),
        42 => Asset::new(
            42,
            [24.0, 24.0],
            [5.0, 9.0, 14.0, 9.0],
            [5.234375, 9.7578125, 13.5078, 7.5078373],
        ),
        _ => return false,
    };
    asset.gray_white = matches!(number, 16 | 49);
    paint_asset(ui, rect, asset, false)
}

fn paint_asset(ui: &Ui, rect: Rect, asset: Asset, is_mode: bool) -> bool {
    let Some(data) = bytes(asset.number, asset.gray_white) else {
        return false;
    };
    let remaining = (rect.size() - asset.component.size()) * 0.5;
    let offset = Vec2::new(remaining.x.trunc(), remaining.y.trunc());
    let (origin, scale) = if is_mode {
        (rect.min + offset - asset.component.min.to_vec2(), 1.0)
    } else {
        // Fit and center the drawable proportionally in the integer rectangle.
        let target = Rect::from_min_size(rect.min + offset, rect.size() - 2.0 * offset);
        let scale = (target.width() / asset.drawable.width())
            .min(target.height() / asset.drawable.height());
        (
            target.center() - asset.drawable.center().to_vec2() * scale,
            scale,
        )
    };
    let opacity = if ui.is_enabled() { 255 } else { 77 };
    egui::Image::from_bytes(
        format!(
            "bytes://reference-ui/image-{:03}-{}.svg",
            asset.number,
            if asset.gray_white {
                "dialog"
            } else {
                "original"
            }
        ),
        data,
    )
    .tint(Color32::from_white_alpha(opacity))
    .paint_at(ui, Rect::from_min_size(origin, asset.viewport * scale));
    true
}
