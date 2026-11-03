//! Native export options with native-only fields preserved in the source object.
use crate::app::{
    export::{Encoding, Format, NamePart, Settings},
    native_adapter::fields::{Field, get},
};
use serde_json::Value;

pub(super) fn decode(value: &Value) -> Result<Settings, String> {
    let path = "/project/export";
    let object = Field { value, path }.object()?;
    let int = |key| {
        Field {
            value: get(object, key, path)?,
            path,
        }
        .signed()
    };
    let boolean = |key| {
        Field {
            value: get(object, key, path)?,
            path,
        }
        .boolean()
    };
    let split = match int("mode")? {
        0 => false,
        1 => true,
        _ => {
            return Err(
                "Native explicit-block export mode has no stored project export equivalent".into(),
            );
        }
    };
    let format = match int("audio-format")? {
        0 => Format::Wav,
        2 => Format::Flac,
        _ => return Err("Unsupported native audio format".into()),
    };
    let rate = Field {
        value: get(object, "sample-rate", path)?,
        path,
    }
    .unsigned()?;
    let encoding = match int("char-code")? {
        0 => Encoding::Utf8,
        1 => Encoding::ShiftJis,
        _ => return Err("Unsupported native character code".into()),
    };
    boolean("name-rule")?;
    let raw = Field {
        value: get(object, "name-formats", path)?,
        path,
    }
    .array(3)?;
    if raw.len() != 3 {
        return Err("Native export requires three naming fields".into());
    }
    let mut parts = [NamePart::None; 3];
    for (part, value) in parts.iter_mut().zip(raw) {
        *part = match (Field { value, path }).signed()? {
            0 => NamePart::None,
            1 => NamePart::Number,
            2 => NamePart::Voice,
            3 => NamePart::Script,
            _ => return Err("Unsupported native name part".into()),
        };
    }
    let result = Settings {
        format,
        rate,
        encoding,
        split,
        parts,
        scripts: boolean("write-scripts")?,
        text: boolean("write-text")?,
        srt: boolean("write-srt")?,
        lab: boolean("write-lab")?,
        ..Settings::default()
    };
    result.validate_stored()?;
    Ok(result)
}

pub(super) fn overlay(source: &Value, settings: &Settings) -> Result<Value, String> {
    settings.validate_stored()?;
    let mut value = source.clone();
    let object = value
        .as_object_mut()
        .ok_or("Native export is not an object")?;
    object.insert(
        "mode".into(),
        Value::from(if settings.split { 1 } else { 0 }),
    );
    object.insert(
        "audio-format".into(),
        match settings.format {
            Format::Wav => 0,
            Format::Flac => 2,
        }
        .into(),
    );
    object.insert("sample-rate".into(), settings.rate.into());
    object.insert(
        "char-code".into(),
        match settings.encoding {
            Encoding::Utf8 => 0,
            Encoding::ShiftJis => 1,
        }
        .into(),
    );
    for (key, flag) in [
        ("write-scripts", settings.scripts),
        ("write-text", settings.text),
        ("write-srt", settings.srt),
        ("write-lab", settings.lab),
    ] {
        object.insert(key.into(), flag.into());
    }
    object.insert(
        "name-formats".into(),
        Value::Array(
            settings
                .parts
                .iter()
                .map(|part| {
                    match part {
                        NamePart::None => 0,
                        NamePart::Number => 1,
                        NamePart::Voice => 2,
                        NamePart::Script => 3,
                    }
                    .into()
                })
                .collect(),
        ),
    );
    // name-rule is a separate native option; Save-as name is not a native field.
    Ok(value)
}
