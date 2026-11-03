//! Retained crop transform for standing artwork.
use crate::app::catalog::Portrait;
use eframe::egui::{Pos2, Rect, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct View {
    pub scale: [f32; 2],
    pub translation: [f32; 2],
    pub width: i32,
}

impl View {
    pub fn initial(card: Rect, portrait: &Portrait) -> Self {
        let width = card.width().max(0.0) as i32;
        let mut value = Self {
            scale: [1.0; 2],
            translation: [0.0; 2],
            width,
        };
        if let Some([x, y, crop_width, _]) = portrait.default_region {
            if crop_width > 0 {
                let scale = width as f32 / crop_width as f32;
                value.scale = [scale; 2];
                value.translation = [
                    x.wrapping_neg() as f32 * scale + super::super::INSET,
                    y.wrapping_neg() as f32 * scale + super::super::INSET,
                ];
            }
        }
        value.clamp(card, portrait);
        value
    }

    pub fn valid(&self) -> bool {
        self.width >= 0
            && self.scale.iter().all(|v| v.is_finite() && *v > 0.0)
            && self.translation.iter().all(|v| v.is_finite())
    }

    pub fn valid_for(&self, portrait: &Portrait) -> bool {
        self.valid()
            && (self.scale[0] + self.scale[1]).is_finite()
            && (0..2).all(|axis| {
                let extent = self.scale[axis] * portrait.pixel_size[axis] as f32;
                extent.is_finite() && (self.translation[axis] + extent).is_finite()
            })
    }

    pub fn restore_size(&mut self, card: Rect, portrait: &Portrait) {
        let width = card.width().max(0.0) as i32;
        self.translation[0] += (width as f32 - self.width as f32) * 0.5;
        self.width = width;
        self.clamp(card, portrait);
    }

    pub fn resize(&mut self, card: Rect, portrait: &Portrait) {
        let width = card.width().max(0.0) as i32;
        self.translation[0] += ((width - self.width) / 2) as f32;
        self.width = width;
        self.clamp(card, portrait);
    }

    pub fn translate(&mut self, delta: [i32; 2], card: Rect, portrait: &Portrait) {
        self.translation[0] += delta[0] as f32;
        self.translation[1] += delta[1] as f32;
        self.clamp(card, portrait);
    }

    pub fn zoom(&mut self, delta_y: f32, point: [i32; 2], card: Rect, portrait: &Portrait) {
        let mut factor = delta_y / 3.0 + 1.0;
        let minimum = (card.width() / portrait.pixel_size[0] as f32)
            .max(card.height() / portrait.pixel_size[1] as f32);
        let scale = (self.scale[0].abs() + self.scale[1].abs()) * 0.5;
        if minimum > scale * factor {
            factor = minimum / scale;
        }
        if scale * factor > 1.5 {
            factor = 1.5 / scale;
        }
        for (axis, point) in point.into_iter().enumerate() {
            self.scale[axis] *= factor;
            self.translation[axis] =
                (self.translation[axis] - point as f32) * factor + point as f32;
        }
        self.clamp(card, portrait);
    }

    pub fn clamp(&mut self, card: Rect, portrait: &Portrait) {
        let size = Vec2::new(portrait.pixel_size[0] as f32, portrait.pixel_size[1] as f32);
        let extent = Vec2::new(size.x * self.scale[0], size.y * self.scale[1]);
        let ratio = (card.width() / extent.x).max(card.height() / extent.y);
        if ratio > 1.0 {
            let center = [
                super::super::INSET + card.width() * 0.5,
                super::super::INSET + card.height() * 0.5,
            ];
            for (axis, center) in center.into_iter().enumerate() {
                self.scale[axis] *= ratio;
                self.translation[axis] = (self.translation[axis] - center) * ratio + center;
            }
        }
        let inset = super::super::INSET;
        if self.translation[0] > inset {
            self.translation[0] += inset - self.translation[0];
        }
        if self.translation[1] > inset {
            self.translation[1] += inset - self.translation[1];
        }
        let right = self.translation[0] + size.x * self.scale[0];
        if inset + card.width() > right {
            self.translation[0] += inset + card.width() - right;
        }
        let bottom = self.translation[1] + size.y * self.scale[1];
        if inset + card.height() > bottom {
            self.translation[1] += inset + card.height() - bottom;
        }
    }

    pub fn image_rect(&self, panel: Rect, portrait: &Portrait) -> Rect {
        Rect::from_min_size(
            Pos2::new(
                panel.left() + self.translation[0],
                panel.top() + self.translation[1],
            ),
            Vec2::new(
                portrait.pixel_size[0] as f32 * self.scale[0],
                portrait.pixel_size[1] as f32 * self.scale[1],
            ),
        )
    }
}
