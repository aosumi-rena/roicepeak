use super::{Entry, POS, file};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const NAMES: [&str; 6] = [
    "Japanese_Futsuu_meishi",
    "Japanese_Koyuumeishi_ippan",
    "Japanese_Koyuumeishi_jinmei",
    "Japanese_Koyuumeishi_sei",
    "Japanese_Koyuumeishi_mei",
    "Japanese_Koyuumeishi_place",
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Native {
    pub original: Option<Value>,
    pub opaque: bool,
    pub overwrite_accents: Vec<i32>,
}

fn integer(value: &Value) -> Option<i32> {
    value.as_i64().and_then(|v| v.try_into().ok())
}

fn decimal_prefix(value: &str) -> i32 {
    let bytes = value.as_bytes();
    let mut cursor = 0;
    while bytes
        .get(cursor)
        .is_some_and(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | 11 | 12))
    {
        cursor += 1;
    }
    let negative = bytes.get(cursor) == Some(&b'-');
    if negative || bytes.get(cursor) == Some(&b'+') {
        cursor += 1;
    }
    let limit = if negative {
        2_147_483_648u32
    } else {
        2_147_483_647u32
    };
    let mut magnitude = 0u32;
    while let Some(digit) = bytes.get(cursor).filter(|c| c.is_ascii_digit()) {
        magnitude = magnitude
            .saturating_mul(10)
            .saturating_add(u32::from(*digit - b'0'))
            .min(limit);
        cursor += 1;
    }
    if negative {
        0u32.wrapping_sub(magnitude) as i32
    } else {
        magnitude as i32
    }
}

fn converted_integer(value: &Value) -> Result<i32, String> {
    match value {
        Value::Bool(v) => Ok(i32::from(*v)),
        Value::String(v) => Ok(decimal_prefix(v)),
        Value::Number(v) => {
            if let Some(v) = v.as_i64() {
                return Ok(v as i32);
            }
            if v.is_u64() {
                return Err("Dictionary integer exceeds the supported signed range".into());
            }
            let v = v.as_f64().ok_or("Invalid dictionary number")?.trunc();
            Ok(if v < f64::from(i32::MIN) || v > f64::from(i32::MAX) {
                i32::MIN
            } else {
                v as i32
            })
        }
        Value::Null | Value::Array(_) | Value::Object(_) => Ok(0),
    }
}

pub(super) fn decode(value: Value) -> Result<Entry, String> {
    let mut entry = Entry::default();
    entry.native.original = Some(value);
    entry.native.opaque = true;
    let Some(object) = entry.native.original.as_ref().and_then(Value::as_object) else {
        return Ok(entry);
    };
    match object.get("lang") {
        None => return Ok(entry),
        Some(Value::String(value)) if value != "ja" => return Ok(entry),
        Some(_) => {}
    }
    let Some(word) = object.get("sur").and_then(Value::as_str) else {
        return Ok(entry);
    };
    entry.word = word.into();
    let Some(reading) = object.get("pron").and_then(Value::as_str) else {
        return Ok(entry);
    };
    entry.reading = reading.into();
    let Some(pos) = object.get("pos").and_then(Value::as_str) else {
        return Ok(entry);
    };
    let Some(pos) = NAMES
        .iter()
        .zip(POS)
        .position(|(name, japanese)| pos == *name || pos == japanese)
    else {
        return Ok(entry);
    };
    entry.part_of_speech = pos;
    let Some(priority) = object
        .get("priority")
        .and_then(integer)
        .filter(|v| (1..=10).contains(v))
    else {
        return Ok(entry);
    };
    entry.priority = priority as u8;
    let Some(accent) = object
        .get("accentType")
        .and_then(integer)
        .filter(|v| *v >= 0)
    else {
        return Ok(entry);
    };
    entry.accent = accent as usize;
    if let Some(values) = object.get("overwriteAccents").and_then(Value::as_array) {
        if values.len() > 4096 {
            return Err("Dictionary accent override count exceeds 4096".into());
        }
        entry.native.overwrite_accents = values
            .iter()
            .map(converted_integer)
            .collect::<Result<_, _>>()?;
    }
    entry.native.opaque = false;
    entry.validate_stored()?;
    Ok(entry)
}

impl Entry {
    pub(crate) fn validate_stored(&self) -> Result<(), String> {
        if self.word.len() > 4096
            || self.reading.len() > 4096
            || self.part_of_speech >= 6
            || !(1..=10).contains(&self.priority)
            || self.accent > i32::MAX as usize
            || self.native.overwrite_accents.len() > 4096
        {
            return Err("Dictionary entry exceeds its text or control limits".into());
        }
        if self.native.opaque && self.native.original.is_none() {
            return Err("Opaque dictionary entry has no source value".into());
        }
        if self.native.original.is_none()
            && (self.word.trim().is_empty() || self.reading.trim().is_empty() || self.accent > 4096)
        {
            return Err("Invalid project dictionary entry".into());
        }
        if let Some(value) = &self.native.original {
            file::bounded(value)?;
        }
        Ok(())
    }

    pub(super) fn native_value(&self) -> Result<Value, String> {
        self.validate_stored()?;
        if self.native.opaque {
            return self
                .native
                .original
                .clone()
                .ok_or("Missing dictionary source".into());
        }
        let mut object = self
            .native
            .original
            .as_ref()
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_else(Map::new);
        object.insert("sur".into(), self.word.clone().into());
        object.insert("pron".into(), self.reading.clone().into());
        object.insert("pos".into(), NAMES[self.part_of_speech].into());
        object.insert("priority".into(), self.priority.into());
        object.insert("accentType".into(), (self.accent as i32).into());
        if self.native.overwrite_accents.is_empty() {
            object.remove("overwriteAccents");
        } else {
            object.insert(
                "overwriteAccents".into(),
                Value::Array(
                    self.native
                        .overwrite_accents
                        .iter()
                        .copied()
                        .map(Value::from)
                        .collect(),
                ),
            );
        }
        object.insert("lang".into(), "ja".into());
        Ok(Value::Object(object))
    }

    pub(crate) fn estimated_heap_bytes(&self) -> usize {
        fn value(v: &Value) -> usize {
            match v {
                Value::String(v) => v.capacity(),
                Value::Array(v) => v.iter().fold(
                    v.capacity().saturating_mul(std::mem::size_of::<Value>()),
                    |sum, v| sum.saturating_add(value(v)),
                ),
                Value::Object(v) => v.iter().fold(0usize, |sum, (key, v)| {
                    sum.saturating_add(
                        64 + std::mem::size_of::<String>() + std::mem::size_of::<Value>(),
                    )
                    .saturating_add(key.capacity())
                    .saturating_add(value(v))
                }),
                _ => 0,
            }
        }
        self.word
            .capacity()
            .saturating_add(self.reading.capacity())
            .saturating_add(self.native.overwrite_accents.capacity().saturating_mul(4))
            .saturating_add(self.native.original.as_ref().map_or(0, value))
    }
}
