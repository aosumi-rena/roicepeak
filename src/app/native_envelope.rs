//! Extend only explicitly needed typed native envelope records.
use crate::app::{
    document::{Parameters, Project},
    render::Config,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn zero_emotions(source: &Value) -> Result<Value, String> {
    let pool = source.as_object().ok_or("Native emotion pool missing")?;
    if pool.len() > 1024
        || pool
            .keys()
            .any(|key| key.is_empty() || key.len() > 4096 || key.contains('\0'))
    {
        return Err("Native emotion pool exceeds its supported limits".into());
    }
    Ok(Value::Object(
        pool.keys()
            .map(|key| (key.clone(), Value::from(0.0)))
            .collect(),
    ))
}

pub(super) fn extend(
    root: &mut Value,
    project: &Project,
    config: Option<&Config>,
) -> Result<BTreeMap<String, Parameters>, String> {
    let declared = root["voices"]
        .as_object()
        .ok_or("Native voice envelope missing")?
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    let needed = project
        .blocks
        .iter()
        .map(|block| block.narrator.identity.clone())
        .chain(project.narrator_settings.keys().cloned())
        .collect::<BTreeSet<_>>();
    let missing = needed.difference(&declared).cloned().collect::<Vec<_>>();
    let mut corrections = project.narrator_settings.clone();
    let new_blocks = project
        .blocks
        .iter()
        .filter(|block| block.native_owner.is_none())
        .count();
    let existing_emotion_rows = root["project"]["global-emotions"]
        .as_array()
        .ok_or("Native correction list missing")?
        .iter()
        .filter_map(|row| row["nar"].as_str())
        .collect::<BTreeSet<_>>();
    let needs_pool = !missing.is_empty()
        || new_blocks != 0
        || corrections
            .keys()
            .any(|identity| !existing_emotion_rows.contains(identity.as_str()));
    if !needs_pool {
        return Ok(corrections);
    }
    let config = config.ok_or("Native Save requires available narrator resources")?;
    if config.selections.is_empty() || config.selections.len() > 256 {
        return Err("Native Save requires 1 to 256 catalog voice sources".into());
    }
    if !missing.is_empty()
        && declared
            .len()
            .checked_add(missing.len())
            .is_none_or(|n| n > 256)
    {
        return Err("Native voice envelope exceeds the supported count".into());
    }
    let pool = root["project"]["emotions"]
        .as_object()
        .ok_or("Native emotion pool missing")?;
    let mut keys = pool.keys().cloned().collect::<BTreeSet<_>>();
    let mut versions = BTreeMap::<String, i32>::new();
    for source in &config.selections {
        if source.identity.is_empty()
            || source.identity.len() > 4096
            || source.identity.contains('\0')
        {
            return Err("Invalid native narrator identity".into());
        }
        let version =
            i32::try_from(source.version).map_err(|_| "Native catalog version exceeds int32")?;
        versions
            .entry(source.identity.clone())
            .and_modify(|old| *old = (*old).max(version))
            .or_insert(version);
        let binding = config
            .voices
            .get(&source.binding_key)
            .ok_or("Catalog voice binding is missing")?;
        keys.extend(
            binding
                .emotion_bindings
                .iter()
                .map(|emotion| emotion.key.clone()),
        );
    }
    if keys.len() > 1024
        || keys
            .iter()
            .any(|key| key.is_empty() || key.len() > 4096 || key.contains('\0'))
    {
        return Err("Native emotion pool exceeds its supported limits".into());
    }
    let zero = Value::Object(
        keys.iter()
            .map(|key| (key.clone(), Value::from(0.0)))
            .collect(),
    );
    let correction_copies = corrections
        .keys()
        .chain(missing.iter())
        .filter(|identity| !existing_emotion_rows.contains(identity.as_str()))
        .collect::<BTreeSet<_>>()
        .len();
    let copies = new_blocks
        .checked_add(correction_copies)
        .and_then(|n| n.checked_add(1))
        .ok_or("Native emotion pool size overflow")?;
    if copies.checked_mul(keys.len()).is_none_or(|n| n > 500_000)
        || copies
            .checked_mul(serde_json::to_vec(&zero).map_err(|e| e.to_string())?.len())
            .is_none_or(|n| n > 16 * 1024 * 1024)
    {
        return Err("New native emotion pools exceed the document budget".into());
    }
    let additions = missing
        .iter()
        .map(|identity| {
            let version = *versions.get(identity).ok_or(
                "Native narrator or correction identity is absent from the current catalog",
            )?;
            Ok((identity.clone(), json!({"nid":identity,"latest":version})))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let voices = root["voices"].as_object_mut().unwrap();
    for (identity, record) in additions {
        voices.insert(identity.clone(), record);
        corrections.entry(identity).or_default();
    }
    let pool = root["project"]["emotions"].as_object_mut().unwrap();
    for key in keys {
        pool.entry(key).or_insert_with(|| Value::from(0.0));
    }
    Ok(corrections)
}
