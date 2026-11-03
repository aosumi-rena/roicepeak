//! Canonical normalized control values; percentages belong to editor controls.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, Default)]
#[repr(transparent)]
pub struct ControlValue(f32);

impl ControlValue {
    pub const ZERO: Self = Self(0.0);
    pub const ONE: Self = Self(1.0);
    pub const fn from_normalized(value: f32) -> Self {
        Self(value)
    }
    pub fn from_percent(value: f32) -> Self {
        Self(value / 100.0)
    }
    pub const fn normalized(self) -> f32 {
        self.0
    }
    pub fn percent(self) -> f32 {
        self.0 * 100.0
    }
    pub fn is_finite(self) -> bool {
        self.0.is_finite()
    }
    pub fn displayable(self) -> bool {
        self.is_finite() && self.percent().is_finite()
    }
    pub fn bits(self) -> u32 {
        self.0.to_bits()
    }
    pub fn same_number(self, other: Self) -> bool {
        self.0 == other.0
    }
}

// Source identity and history must distinguish every retained native F32 word.
impl PartialEq for ControlValue {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}
impl Eq for ControlValue {}
impl Hash for ControlValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}
impl Serialize for ControlValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f32(self.0)
    }
}
impl<'de> Deserialize<'de> for ControlValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f32::deserialize(deserializer)?;
        if !value.is_finite() {
            return Err(de::Error::custom("Control must be finite float32"));
        }
        Ok(Self(value))
    }
}
