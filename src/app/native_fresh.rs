#[path = "native_fresh_words.rs"]
mod words;

use super::{emotion_object, options, scalar_object};
use crate::app::{
    document::{Block, OffsetMode, Parameters, Project, SentenceState, VoiceVersion},
    native_project::{NativeProject, TARGET_VERSION},
    render::Config,
    sentences,
};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn new_sentence(sentence: &SentenceState) -> Result<Value, String> {
    if sentence.native_owner.is_some() {
        return Err("A new native sentence cannot reuse a retained owner".into());
    }
    replacement_sentence(sentence)
}

pub(super) fn replacement_sentence(sentence: &SentenceState) -> Result<Value, String> {
    if sentence.text.trim().is_empty() {
        if sentence.ssml.is_some() || sentence.words.iter().any(|word| word.manual()) {
            return Err(
                "Whitespace-only text cannot retain authored pronunciation controls".into(),
            );
        }
        return Ok(json!({"text":sentence.text,"has-eos":true,"tokens":[]}));
    }
    words::replacement(sentence)
}

fn block(block: &Block, config: &Config, emotions: &Value) -> Result<Value, String> {
    if block.native_owner.is_some()
        || block
            .sentences
            .iter()
            .any(|sentence| sentence.native_owner.is_some())
    {
        return Err("A new native document cannot contain retained native owners".into());
    }
    config.binding(&block.narrator)?;
    let partition = sentences::partition(&block.text);
    let list = if block.text.trim().is_empty() {
        if block.sentences.iter().any(|sentence| {
            sentence.ssml.is_some() || sentence.words.iter().any(|word| word.manual())
        }) || block.segments.iter().any(|segment| {
            segment.accent_edited
                || segment.intonation_delta != 0.0
                || segment.duration_edit.edited()
        }) {
            return Err(
                "Whitespace-only text cannot retain authored pronunciation controls".into(),
            );
        }
        partition
            .iter()
            .map(|range| {
                json!({"text":&block.text[range.bytes.clone()],
            "has-eos":true,"tokens":[]})
            })
            .collect::<Vec<_>>()
    } else {
        if partition.len() != block.sentences.len()
            || partition
                .iter()
                .zip(&block.sentences)
                .any(|(range, sentence)| {
                    block.text.get(range.bytes.clone()) != Some(sentence.text.as_str())
                })
        {
            return Err("Native Save requires complete prepared sentence boundaries".into());
        }
        block
            .sentences
            .iter()
            .map(words::sentence)
            .collect::<Result<Vec<_>, _>>()?
    };
    let ranges = partition
        .iter()
        .map(|range| {
            Ok(json!([
                i32::try_from(range.chars.start)
                    .map_err(|_| "Native sentence position exceeds int32")?,
                i32::try_from(range.chars.end)
                    .map_err(|_| "Native sentence position exceeds int32")?,
            ]))
        })
        .collect::<Result<Vec<Value>, String>>()?;
    let mut raw = block_shell(block, config, emotions)?;
    raw["sentence-list"] = Value::Array(list);
    raw["sentence-ranges"] = Value::Array(ranges);
    Ok(raw)
}

pub(super) fn block_shell(
    block: &Block,
    config: &Config,
    emotions: &Value,
) -> Result<Value, String> {
    if block.native_owner.is_some() {
        return Err("A new native block cannot reuse a retained owner".into());
    }
    config.binding(&block.narrator)?;
    let pool = emotions.as_object().ok_or("Native emotion pool missing")?;
    if block
        .parameters
        .emotions
        .keys()
        .any(|key| !pool.contains_key(key))
    {
        return Err("New native emotion controls require declared pool keys".into());
    }
    let list = Vec::<Value>::new();
    let ranges = Vec::<Value>::new();
    let version = match block.narrator.version {
        VoiceVersion::Latest => -1,
        VoiceVersion::Exact(value) => {
            i32::try_from(value).map_err(|_| "Native narrator version exceeds int32")?
        }
    };
    Ok(
        json!({"narrator":{"key":block.narrator.identity,"language":block.narrator.language,"narrator-version":version},
        "time-offset-mode":match block.offset_mode {OffsetMode::Absolute=>0,OffsetMode::PreviousStart=>1,OffsetMode::PreviousEnd=>2},
        "time-offset":block.offset_seconds,"params":scalar_object(&json!({}),&block.parameters)?,
        "emotions":emotion_object(emotions,&block.parameters)?,"sentence-list":list,"sentence-ranges":ranges}),
    )
}

pub(super) fn encode(project: &Project, config: Option<&Config>) -> Result<Vec<u8>, String> {
    if project.native_origin.is_some() {
        return Err("New native serialization requires a project without a native origin".into());
    }
    let config = config.ok_or("Native Save requires available narrator resources")?;
    project.validate_controls(config.value_policy)?;
    if project
        .global
        .emotions
        .values()
        .any(|value| value.bits() != 0)
    {
        return Err("Authored legacy global emotions have no native synthesis mapping".into());
    }
    if config.selections.is_empty() || config.selections.len() > 256 {
        return Err("Native Save requires 1 to 256 catalog voice sources".into());
    }
    let mut narrators = BTreeMap::<String, i32>::new();
    let mut keys = BTreeSet::<String>::new();
    for source in &config.selections {
        if source.identity.is_empty()
            || source.identity.len() > 4096
            || source.identity.contains('\0')
        {
            return Err("Invalid native narrator identity".into());
        }
        let version =
            i32::try_from(source.version).map_err(|_| "Native catalog version exceeds int32")?;
        narrators
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
    for parameters in std::iter::once(&project.global)
        .chain(project.narrator_settings.values())
        .chain(project.blocks.iter().map(|block| &block.parameters))
    {
        if parameters.emotions.keys().any(|key| !keys.contains(key)) {
            return Err("Native emotion controls require catalog-defined keys".into());
        }
    }
    if keys.len() > 1024
        || keys
            .iter()
            .any(|key| key.is_empty() || key.len() > 4096 || key.contains('\0'))
    {
        return Err("Native emotion pool exceeds its supported limits".into());
    }
    if project
        .narrator_settings
        .keys()
        .any(|identity| !narrators.contains_key(identity))
    {
        return Err("Native narrator corrections require a catalog identity".into());
    }
    let zero = Value::Object(
        keys.into_iter()
            .map(|key| (key, Value::from(0.0)))
            .collect(),
    );
    let copies = project
        .blocks
        .len()
        .checked_add(narrators.len())
        .and_then(|n| n.checked_add(1))
        .ok_or("Native emotion pool size overflow")?;
    let pool_values = copies
        .checked_mul(zero.as_object().unwrap().len())
        .ok_or("Native emotion pool size overflow")?;
    let pool_bytes = copies
        .checked_mul(serde_json::to_vec(&zero).map_err(|e| e.to_string())?.len())
        .ok_or("Native emotion pool size overflow")?;
    if pool_values > 500_000 || pool_bytes > 16 * 1024 * 1024 {
        return Err("Native emotion pools exceed the document budget".into());
    }
    let mut voices = Map::new();
    let (mut settings, mut emotions) = (Vec::new(), Vec::new());
    for (identity, latest) in narrators {
        let parameters = project
            .narrator_settings
            .get(&identity)
            .cloned()
            .unwrap_or_else(Parameters::default);
        voices.insert(identity.clone(), json!({"latest":latest,"nid":identity}));
        settings.push(json!({"nar":identity,"pm":scalar_object(&json!({}),&parameters)?}));
        emotions.push(json!({"nar":identity,"em":emotion_object(&zero,&parameters)?}));
    }
    let blocks = project
        .blocks
        .iter()
        .map(|value| block(value, config, &zero))
        .collect::<Result<Vec<_>, _>>()?;
    let export = options::overlay(&json!({"name-rule":false}), &project.export_settings)?;
    let root = json!({"version":TARGET_VERSION,"voices":voices,"project":{
        "params":scalar_object(&json!({}),&project.global)?,"emotions":zero,
        "global-settings":settings,"global-emotions":emotions,"export":export,"blocks":blocks}});
    let mut bytes = serde_json::to_vec(&root).map_err(|e| e.to_string())?;
    bytes.push(0);
    NativeProject::parse(&bytes)?;
    Ok(bytes)
}
