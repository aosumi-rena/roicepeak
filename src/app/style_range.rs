#[derive(Clone, Copy, PartialEq)]
pub(super) struct Range {
    pub min: f64,
    pub max: f64,
}

impl Range {
    pub fn ratio(self, value: f32) -> f32 {
        ((f64::from(value) - self.min) / (self.max - self.min)).clamp(0.0, 1.0) as f32
    }

    pub fn at(self, ratio: f64) -> f64 {
        self.min + (self.max - self.min) * ratio
    }
}
