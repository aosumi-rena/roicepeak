//! Typed editable import and raw-preserving native save transactions.
#[path = "native_envelope.rs"]
mod envelope;
#[path = "native_fresh.rs"]
mod fresh;
#[path = "native_interop_options.rs"]
mod options;
#[path = "native_structure.rs"]
mod structure;
use crate::app::{
    document::{Block, ControlValue, Parameters, Project, VoiceVersion},
    native_adapter::fields::{Field, get},
    native_adapter::{self, NativeDocumentDraft},
    native_origin::NativeOrigin,
    native_project::NativeProject,
    render::Config,
};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

fn scalars(source: &native_adapter::NativeParameters) -> Parameters {
    let [speed, pitch, pause, volume] = source
        .scalars
        .map(|n| ControlValue::from_normalized(n.value()));
    Parameters {
        speed,
        pitch,
        pause,
        volume,
        emotions: BTreeMap::new(),
    }
}
fn scalar_object(source: &Value, parameters: &Parameters) -> Result<Value, String> {
    let mut result = source
        .as_object()
        .ok_or("Native parameters are not an object")?
        .clone();
    for (key, value) in [
        ("speed", parameters.speed),
        ("pitch", parameters.pitch),
        ("pause", parameters.pause),
        ("volume", parameters.volume),
    ] {
        let unchanged = result
            .get(key)
            .and_then(Value::as_f64)
            .is_some_and(|n| (n as f32).to_bits() == value.bits());
        if !unchanged {
            result.insert(key.into(), Value::from(f64::from(value.normalized())));
        }
    }
    Ok(Value::Object(result))
}
fn emotion_object(source: &Value, parameters: &Parameters) -> Result<Value, String> {
    let mut result = source
        .as_object()
        .ok_or("Native emotions are not an object")?
        .clone();
    for (key, value) in &mut result {
        let current = parameters.emotions.get(key).copied().unwrap_or_default();
        if !value
            .as_f64()
            .is_some_and(|n| (n as f32).to_bits() == current.bits())
        {
            *value = Value::from(f64::from(current.normalized()));
        }
    }
    for (key, value) in &parameters.emotions {
        if !result.contains_key(key) {
            result.insert(key.clone(), Value::from(f64::from(value.normalized())));
        }
    }
    Ok(Value::Object(result))
}
fn corrections(project: &Map<String, Value>) -> Result<BTreeMap<String, Parameters>, String> {
    let mut result = BTreeMap::<String, Parameters>::new();
    for (name, payload) in [("global-settings", "pm"), ("global-emotions", "em")] {
        let path = format!("/project/{name}");
        let rows = Field {
            value: get(project, name, "/project")?,
            path: &path,
        }
        .array(256)?;
        let mut seen = BTreeSet::new();
        for (index, value) in rows.iter().enumerate() {
            let path = format!("{path}/{index}");
            let row = Field { value, path: &path }.object()?;
            let identity = Field {
                value: get(row, "nar", &path)?,
                path: &path,
            }
            .string(4096)?
            .to_owned();
            if identity.is_empty() || !seen.insert(identity.clone()) {
                return Err(format!("{path}: empty or repeated narrator identity"));
            }
            let parameters = result.entry(identity).or_default();
            if payload == "pm" {
                let native = native_adapter::parameters(get(row, payload, &path)?, &path)?;
                let emotions = std::mem::take(&mut parameters.emotions);
                *parameters = scalars(&native);
                parameters.emotions = emotions;
            } else {
                parameters.emotions = native_adapter::emotions(get(row, payload, &path)?, &path)?
                    .into_iter()
                    .map(|(k, n)| (k, ControlValue::from_normalized(n.value())))
                    .collect();
            }
        }
    }
    Ok(result)
}

pub(crate) fn decode(native: NativeProject, config: Option<&Config>) -> Result<Project, String> {
    let draft = NativeDocumentDraft::decode(&native)?;
    let object = Field {
        value: &native.root()["project"],
        path: "/project",
    }
    .object()?;
    let legacy =
        native_adapter::emotions(get(object, "emotions", "/project")?, "/project/emotions")?;
    if legacy.values().any(|v| v.value() != 0.0) {
        return Err(
            "Nonzero native legacy project emotions have no qualified synthesis binding".into(),
        );
    }
    let narrator_settings = corrections(object)?;
    let export_settings = options::decode(get(object, "export", "/project")?)?;
    let origin = NativeOrigin::new(native)?;
    let mut project = Project {
        global: scalars(&draft.global),
        narrator_settings,
        blocks: Vec::new(),
        export_settings,
        native_origin: Some(origin.clone()),
        ..Project::default()
    };
    for (index, source) in draft.blocks().iter().enumerate() {
        config
            .ok_or("Configure resources for the native project's used narrator")?
            .binding(&source.voice)
            .map_err(|e| format!("/project/blocks/{index}/narrator: {e}"))?;
        let mut block = Block::new(index as u64 + 1);
        block.native_owner = Some(origin.block_owner(index)?);
        block.narrator = source.voice.clone();
        block.offset_mode = source.offset_mode;
        block.offset_seconds = source.offset_seconds;
        block.parameters = scalars(&source.parameters);
        block.parameters.emotions = source
            .emotions
            .iter()
            .map(|(k, n)| (k.clone(), ControlValue::from_normalized(n.value())))
            .collect();
        for (si, source) in source.sentences().iter().enumerate() {
            if !source.has_eos {
                return Err(format!(
                    "/project/blocks/{index}/sentence-list/{si}: incomplete sentence query semantics are not qualified"
                ));
            }
            let mut sentence = source.state().clone();
            if sentence.text.is_empty() {
                return Err(
                    "Empty native sentence nodes have no canonical sentence representation".into(),
                );
            }
            if sentence
                .words
                .iter()
                .any(|w| w.language != 1 || matches!(w.pos, 0x2000 | 0x2001))
            {
                return Err(
                    "This editable native profile requires ordinary Japanese canonical words"
                        .into(),
                );
            }
            sentence.native_owner = Some(origin.sentence_owner(index, si)?);
            let length = sentence.text.chars().count();
            let mut ranges = Vec::with_capacity(source.word_ranges.len());
            for range in &source.word_ranges {
                let [a, b] = range.r32;
                if a < 0 || b <= a || b as usize > length {
                    return Err(
                        "Native word scalar range is not usable by the current canonical editor"
                            .into(),
                    );
                }
                ranges.push(Some(a as usize..b as usize));
            }
            sentence.word_positions = crate::app::sentence_edit::WordPositions {
                source: sentence.text.clone(),
                ranges,
            };
            block.text.push_str(&sentence.text);
            block.sentence_serial = sentence.id;
            block.sentences.push(sentence);
        }
        project.blocks.push(block);
    }
    project.validate()?;
    project.validate_controls(config.map_or(Default::default(), |c| c.value_policy))?;
    Ok(project)
}

fn overlay_corrections(
    root: &mut Value,
    parameters: &BTreeMap<String, Parameters>,
) -> Result<(), String> {
    let existing_emotions = root["project"]["global-emotions"]
        .as_array()
        .ok_or("Native correction list missing")?;
    let needs_defaults = parameters.keys().any(|identity| {
        !existing_emotions
            .iter()
            .any(|row| row["nar"].as_str() == Some(identity.as_str()))
    });
    let defaults = if needs_defaults {
        envelope::zero_emotions(&root["project"]["emotions"])?
    } else {
        Value::Object(Map::new())
    };
    let declared = root["voices"]
        .as_object()
        .ok_or("Native voice envelope missing")?
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    for (name, payload) in [("global-settings", "pm"), ("global-emotions", "em")] {
        let rows = root["project"][name]
            .as_array_mut()
            .ok_or("Native correction list missing")?;
        let mut seen = BTreeSet::new();
        for row in rows.iter_mut() {
            let identity = row["nar"]
                .as_str()
                .ok_or("Native correction identity missing")?
                .to_owned();
            if !seen.insert(identity.clone()) {
                return Err("Repeated native correction identity".into());
            }
            let value = parameters.get(&identity).cloned().unwrap_or_default();
            row[payload] = if payload == "pm" {
                scalar_object(&row[payload], &value)?
            } else {
                emotion_object(&row[payload], &value)?
            };
        }
        for (identity, value) in parameters {
            if !seen.contains(identity) {
                if !declared.contains(identity) {
                    return Err("Native Save cannot yet add a correction identity absent from its original voice envelope".into());
                }
                let contents = if payload == "pm" {
                    scalar_object(&serde_json::json!({}), value)?
                } else {
                    emotion_object(&defaults, value)?
                };
                let mut row = Map::new();
                row.insert("nar".into(), identity.clone().into());
                row.insert(payload.into(), contents);
                rows.push(Value::Object(row));
            }
        }
    }
    Ok(())
}

pub(crate) fn requires_preparation(project: &Project) -> Result<bool, String> {
    structure::requires_preparation(project)
}

pub(crate) fn encode(project: &Project, config: Option<&Config>) -> Result<Vec<u8>, String> {
    let mut project = project.clone();
    project.synchronize_sentences()?;
    project.validate()?;
    let Some(origin) = project.native_origin.as_ref() else {
        return fresh::encode(&project, config);
    };
    if project
        .global
        .emotions
        .values()
        .any(|v| v.normalized() != 0.0)
    {
        return Err("Authored legacy global emotions have no qualified native mapping".into());
    }
    let mut root = origin.root().clone();
    root["project"]["params"] = scalar_object(&root["project"]["params"], &project.global)?;
    let corrections = envelope::extend(&mut root, &project, config)?;
    overlay_corrections(&mut root, &corrections)?;
    root["project"]["export"] =
        options::overlay(&root["project"]["export"], &project.export_settings)?;
    let inserted = project
        .blocks
        .iter()
        .filter(|block| block.native_owner.is_none())
        .count();
    let pool = root["project"]["emotions"]
        .as_object()
        .ok_or("Native emotion pool missing")?;
    let pool_values = inserted
        .checked_mul(pool.len())
        .ok_or("Native emotion pool size overflow")?;
    let pool_bytes = inserted
        .checked_mul(serde_json::to_vec(pool).map_err(|e| e.to_string())?.len())
        .ok_or("Native emotion pool size overflow")?;
    if pool_values > 500_000 || pool_bytes > 16 * 1024 * 1024 {
        return Err("Inserted native emotion pools exceed the document budget".into());
    }
    let mut blocks = Vec::with_capacity(project.blocks.len());
    for block in &project.blocks {
        config
            .ok_or("Native Save requires available used narrator bindings")?
            .binding(&block.narrator)?;
        if root["voices"].get(&block.narrator.identity).is_none() {
            return Err("Native Save cannot yet add a narrator absent from the original native voice envelope".into());
        }
        let block_owner = block.native_owner;
        let mut raw = if let Some(owner) = block_owner {
            origin.block(owner)?.clone()
        } else {
            fresh::block_shell(block, config.unwrap(), &root["project"]["emotions"])?
        };
        structure::overlay(origin, block_owner, block, &mut raw)?;
        raw["params"] = scalar_object(&raw["params"], &block.parameters)?;
        raw["emotions"] = emotion_object(&raw["emotions"], &block.parameters)?;
        raw["narrator"]["key"] = block.narrator.identity.clone().into();
        raw["narrator"]["language"] = block.narrator.language.clone().into();
        raw["narrator"]["narrator-version"] = match block.narrator.version {
            VoiceVersion::Latest => Value::from(-1),
            VoiceVersion::Exact(v) => {
                Value::from(i32::try_from(v).map_err(|_| "Native narrator version exceeds int32")?)
            }
        };
        raw["time-offset-mode"] = match block.offset_mode {
            crate::app::document::OffsetMode::Absolute => 0,
            crate::app::document::OffsetMode::PreviousStart => 1,
            crate::app::document::OffsetMode::PreviousEnd => 2,
        }
        .into();
        raw["time-offset"] = Value::from(block.offset_seconds);
        blocks.push(raw);
    }
    root["project"]["blocks"] = Value::Array(blocks);
    let mut bytes = serde_json::to_vec(&root).map_err(|e| e.to_string())?;
    let original_canonical = serde_json::to_vec(origin.root()).map_err(|e| e.to_string())?;
    if bytes == original_canonical {
        return Ok(origin.original_bytes().to_vec());
    }
    bytes.push(0);
    NativeProject::parse(&bytes)?;
    Ok(bytes)
}
