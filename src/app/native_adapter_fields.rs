//! Typed reads of the observed native project schema.
use serde_json::{Map, Value};

#[derive(Clone, Copy)]
pub(crate) struct Field<'a> {
    pub value: &'a Value,
    pub path: &'a str,
}

impl<'a> Field<'a> {
    pub fn error(self, message: &str) -> String {
        format!("{}: {message}", self.path)
    }
    pub fn object(self) -> Result<&'a Map<String, Value>, String> {
        self.value
            .as_object()
            .ok_or_else(|| self.error("expected object"))
    }
    pub fn array(self, limit: usize) -> Result<&'a [Value], String> {
        let values = self
            .value
            .as_array()
            .ok_or_else(|| self.error("expected array"))?;
        if values.len() > limit {
            return Err(self.error("array exceeds adapter limit"));
        }
        Ok(values)
    }
    pub fn string(self, limit: usize) -> Result<&'a str, String> {
        let value = self
            .value
            .as_str()
            .ok_or_else(|| self.error("expected string"))?;
        if value.len() > limit || value.contains('\0') {
            return Err(self.error("string exceeds adapter limit or contains NUL"));
        }
        Ok(value)
    }
    pub fn boolean(self) -> Result<bool, String> {
        self.value
            .as_bool()
            .ok_or_else(|| self.error("expected boolean"))
    }
    pub fn unsigned(self) -> Result<u32, String> {
        self.value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| self.error("expected uint32"))
    }
    pub fn signed(self) -> Result<i32, String> {
        self.value
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| self.error("expected int32"))
    }
    pub fn float64(self) -> Result<f64, String> {
        self.value
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| self.error("expected finite number"))
    }
    pub fn float32(self) -> Result<f32, String> {
        let value = self.float64()? as f32;
        value
            .is_finite()
            .then_some(value)
            .ok_or_else(|| self.error("number cannot be represented by finite float32"))
    }
    pub fn pair(self) -> Result<[i32; 2], String> {
        let values = self.array(2)?;
        if values.len() != 2 {
            return Err(self.error("expected two int32 endpoints"));
        }
        Ok([
            Field {
                value: &values[0],
                path: self.path,
            }
            .signed()?,
            Field {
                value: &values[1],
                path: self.path,
            }
            .signed()?,
        ])
    }
}

pub(crate) fn get<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a Value, String> {
    object
        .get(key)
        .ok_or_else(|| format!("{path}: missing field {key}"))
}

pub(crate) fn child_path(path: &str, key: &str) -> String {
    format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"))
}
