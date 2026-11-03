//! Normalized application controls composed in the native editor's operation order.
//! Direct model and CLI parameters retain their existing calculations by default.
use crate::{Result, invalid};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ValuePolicy {
    pub allow_out_of_range: bool,
    pub allow_negative: bool,
}

impl ValuePolicy {
    pub fn limits(self, min: f32, max: f32) -> (f32, f32) {
        match (self.allow_negative, self.allow_out_of_range) {
            (true, true) => (-2.0, 2.0),
            (true, false) => (-1.0, 1.0),
            (false, true) => (0.0, 2.0),
            (false, false) => (min, max),
        }
    }

    pub fn permits(self, value: f32, min: f32, max: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        let (lower, upper) = self.limits(min, max);
        value >= lower && value <= upper
    }

    pub fn permits_nonnegative(self, value: f32, min: f32, max: f32) -> bool {
        value >= 0.0 && self.permits(value, min, max)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalizedScalars {
    pub speed: f32,
    pub pitch: f32,
    pub pause: f32,
    pub volume: f32,
}

impl Default for NormalizedScalars {
    fn default() -> Self {
        Self {
            speed: 1.0,
            pitch: 0.0,
            pause: 1.0,
            volume: 1.0,
        }
    }
}

impl NormalizedScalars {
    fn validate(self, policy: ValuePolicy) -> Result<()> {
        if [self.speed, self.pitch, self.pause, self.volume]
            .iter()
            .any(|value| !value.is_finite())
            || !policy.permits(self.speed, 0.5, 2.0)
            || !policy.permits(self.pitch, -1.5, 1.5)
            || !policy.permits(self.pause, 0.5, 2.0)
            || !policy.permits_nonnegative(self.volume, 0.0, 2.0)
        {
            return Err(invalid("normalized scalar layer outside native UI ranges"));
        }
        if self.speed == 0.0 {
            return Err(invalid("speed cannot be zero for synthesis"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScalarLayers {
    pub block: NormalizedScalars,
    pub project: NormalizedScalars,
    pub narrator: NormalizedScalars,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedScalars {
    pub speed_duration_factor: f64,
    pub pause_duration_factor: f64,
    pub pitch_offset: f32,
    pub volume_gain: f64,
}

impl ScalarLayers {
    pub fn resolve(self) -> Result<ResolvedScalars> {
        self.resolve_with_policy(ValuePolicy::default())
    }
    pub fn resolve_with_policy(self, policy: ValuePolicy) -> Result<ResolvedScalars> {
        for layer in [self.block, self.project, self.narrator] {
            layer.validate(policy)?;
        }
        // Promote each layer separately; do not clamp the combined speed/pause.
        let speed_duration_factor = (1.0 / f64::from(self.block.speed))
            * (1.0 / f64::from(self.project.speed))
            * (1.0 / f64::from(self.narrator.speed));
        let pause_duration_factor = (f64::from(self.block.pause) * f64::from(self.project.pause))
            * f64::from(self.narrator.pause);
        // Preserve sequential float32 pitch accumulation.
        let semitone = f32::from_bits(0x3daa_aaab);
        let mut pitch_offset = 0.0f32;
        pitch_offset += self.block.pitch * semitone;
        pitch_offset += self.project.pitch * semitone;
        pitch_offset += self.narrator.pitch * semitone;
        // Preserve the native gain limit unless extensions are enabled.
        let volume_product = (f64::from(self.block.volume) * f64::from(self.project.volume))
            * f64::from(self.narrator.volume);
        let volume_gain = if policy.allow_out_of_range {
            volume_product
        } else {
            volume_product.clamp(0.0, 2.0)
        };
        if !speed_duration_factor.is_finite()
            || !pause_duration_factor.is_finite()
            || !pitch_offset.is_finite()
            || !volume_gain.is_finite()
        {
            return Err(invalid("non-finite resolved synthesis controls"));
        }
        Ok(ResolvedScalars {
            speed_duration_factor,
            pause_duration_factor,
            pitch_offset,
            volume_gain,
        })
    }
}
