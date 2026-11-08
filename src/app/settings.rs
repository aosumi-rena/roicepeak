use crate::app::{editor::Mode, resources::Locations};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub font_points: f32,
    pub output_id: String,
    pub global_volume: f32,
    pub loop_playback: bool,
    pub mode: Option<Mode>,
    pub window: Window,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            font_points: crate::app::editor::script_layout::DEFAULT_EM,
            output_id: String::new(),
            global_volume: 100.0,
            loop_playback: false,
            mode: None,
            window: Window::default(),
        }
    }
}

impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        if !self.font_points.is_finite()
            || !(8.0..=72.0).contains(&self.font_points)
            || !self.global_volume.is_finite()
            || !(0.0..=100.0).contains(&self.global_volume)
            || self.output_id.len() > 4096
            || self.output_id.contains('\0')
            || !self.window.valid()
        {
            return Err("Invalid application preferences".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Window {
    pub size: [f32; 2],
    pub position: Option<[f32; 2]>,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            size: [1280.0, 768.0],
            position: None,
            maximized: false,
            fullscreen: false,
        }
    }
}

impl Window {
    fn valid(&self) -> bool {
        self.size.iter().all(|value| value.is_finite())
            && (900.0..=32768.0).contains(&self.size[0])
            && (600.0..=32768.0).contains(&self.size[1])
            && self.position.is_none_or(|point| {
                point
                    .iter()
                    .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
            })
    }

    pub fn viewport(&self, mut builder: egui::ViewportBuilder) -> egui::ViewportBuilder {
        builder = builder
            .with_inner_size(self.size)
            .with_maximized(false)
            .with_fullscreen(false);
        if let Some(position) = self.position {
            builder = builder.with_position(position);
        }
        builder
    }

    pub fn observe(&mut self, ctx: &egui::Context) {
        ctx.input(|input| {
            let info = input.viewport();
            if info.minimized == Some(true) {
                return;
            }
            if let Some(value) = info.maximized {
                self.maximized = value;
            }
            if let Some(value) = info.fullscreen {
                self.fullscreen = value;
            }
            if self.maximized || self.fullscreen {
                return;
            }
            let mut next = self.clone();
            if let Some(rect) = info.inner_rect {
                next.size = [rect.width(), rect.height()];
            } else {
                let size = input.content_rect().size();
                next.size = [size.x, size.y];
            }
            if let Some(rect) = info.outer_rect {
                next.position = Some([rect.min.x, rect.min.y]);
            }
            if next.valid() {
                *self = next;
            }
        });
    }
}

#[derive(Default)]
pub struct State {
    saved: Option<Locations>,
    pending: Option<(Locations, Instant)>,
    failed: Option<Locations>,
    window_restore: Option<(Window, Option<Instant>)>,
}

impl State {
    pub fn loaded(&mut self, locations: &Locations) {
        self.saved = Some(locations.clone());
        self.pending = None;
        self.failed = None;
        let window = &locations.editor.window;
        self.window_restore =
            (window.maximized || window.fullscreen).then(|| (window.clone(), None));
    }

    pub fn observe_window(&mut self, window: &mut Window, ctx: &egui::Context, visible: bool) {
        if let Some((target, requested)) = &mut self.window_restore {
            if let Some(started) = requested {
                let restored = ctx.input(|input| {
                    let info = input.viewport();
                    info.fullscreen == Some(target.fullscreen)
                        && (target.fullscreen || info.maximized == Some(target.maximized))
                });
                if !restored && started.elapsed() < Duration::from_secs(1) {
                    ctx.request_repaint_after(Duration::from_millis(16));
                    return;
                }
                self.window_restore = None;
            } else {
                // Maximizing a hidden Win32 window can reveal it before its first paint.
                if visible {
                    if target.fullscreen {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
                    }
                    *requested = Some(Instant::now());
                }
                ctx.request_repaint_after(Duration::from_millis(16));
                return;
            }
        }
        window.observe(ctx);
    }

    pub fn synchronize(
        &mut self,
        locations: &Locations,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if self.saved.is_none() || self.saved.as_ref() == Some(locations) {
            self.pending = None;
            return Ok(());
        }
        if self.failed.as_ref() == Some(locations) {
            return Ok(());
        }
        let now = Instant::now();
        if self
            .pending
            .as_ref()
            .is_none_or(|(value, _)| value != locations)
        {
            self.pending = Some((locations.clone(), now));
        }
        let elapsed = now.duration_since(self.pending.as_ref().unwrap().1);
        let delay = Duration::from_millis(250);
        if elapsed < delay {
            ctx.request_repaint_after(delay - elapsed);
            return Ok(());
        }
        self.flush(locations)
    }

    pub fn flush(&mut self, locations: &Locations) -> Result<(), String> {
        self.pending = None;
        if self.saved.is_none() || self.saved.as_ref() == Some(locations) {
            return Ok(());
        }
        match locations.save() {
            Ok(()) => {
                self.saved = Some(locations.clone());
                self.failed = None;
                Ok(())
            }
            Err(error) => {
                self.failed = Some(locations.clone());
                Err(format!("Cannot save settings: {error}"))
            }
        }
    }
}
