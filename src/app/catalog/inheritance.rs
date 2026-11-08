use super::{metadata, resolve};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

pub(super) struct Record {
    pub(super) root: PathBuf,
    pub(super) source: PathBuf,
    pub(super) bytes: Arc<Vec<u8>>,
    fields: Map<String, Value>,
    base: Option<String>,
    aliases: Vec<String>,
}

pub(super) struct Resolved {
    pub(super) fields: Map<String, Value>,
    pub(super) network_root: PathBuf,
    pub(super) components_root: PathBuf,
    pub(super) entrance_root: PathBuf,
    pub(super) dependencies: Vec<(PathBuf, Vec<u8>)>,
    pub(super) derived: bool,
}

impl Record {
    pub(super) fn new(root: PathBuf, source: PathBuf, bytes: Vec<u8>) -> Result<Self, String> {
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let fields = value
            .as_object()
            .ok_or("Voice description must be an object")?
            .clone();
        if fields.get("format").and_then(Value::as_u64) != Some(1) {
            return Err("Voice description requires format 1".into());
        }
        let name = fields
            .get("name")
            .and_then(Value::as_str)
            .ok_or("Voice description lacks a name")?;
        metadata::text(name)?;
        let base = match fields.get("base") {
            None | Some(Value::Null) => None,
            Some(Value::String(base)) => {
                metadata::text(base)?;
                Some(base.clone())
            }
            _ => return Err("Voice base must be a narrator identity or null".into()),
        };
        let mut aliases = vec![name.to_owned()];
        if base.is_none()
            && let Some(encoded) = fields
                .get("entrance")
                .and_then(|v| v.get("files"))
                .and_then(|v| v.get("pub/rc/idc.json"))
                .and_then(Value::as_str)
        {
            let manifest: Value = serde_json::from_slice(&resolve::decode(encoded, 65_536)?)
                .map_err(|e| format!("Narrator metadata: {e}"))?;
            if let Some(identity) = manifest.get("narrator_id").and_then(Value::as_str) {
                metadata::text(identity)?;
                aliases.push(identity.to_owned());
            }
        }
        Ok(Self {
            root,
            source,
            bytes: Arc::new(bytes),
            fields,
            base,
            aliases,
        })
    }
}

pub(super) struct Resolver<'a> {
    records: &'a [Record],
    names: BTreeMap<&'a str, usize>,
}

impl<'a> Resolver<'a> {
    pub(super) fn new(records: &'a [Record]) -> Self {
        let mut names = BTreeMap::new();
        for (index, record) in records.iter().enumerate() {
            for name in &record.aliases {
                names.entry(name.as_str()).or_insert(index);
            }
        }
        Self { records, names }
    }

    pub(super) fn resolve(&self, index: usize) -> Result<Resolved, String> {
        self.visit(index, &mut Vec::new())
    }

    fn visit(&self, index: usize, chain: &mut Vec<usize>) -> Result<Resolved, String> {
        if chain.contains(&index) {
            return Err("Voice inheritance contains a cycle".into());
        }
        if chain.len() >= 32 {
            return Err("Voice inheritance depth limit".into());
        }
        let record = &self.records[index];
        chain.push(index);
        let mut resolved = if let Some(base) = &record.base {
            let parent = *self
                .names
                .get(base.as_str())
                .ok_or_else(|| format!("Voice base {base:?} is not in the configured sources"))?;
            let mut resolved = self.visit(parent, chain)?;
            let donor = &self.records[parent];
            resolved
                .dependencies
                .push((donor.source.clone(), donor.bytes.as_ref().clone()));
            resolved
        } else {
            Resolved {
                fields: Map::new(),
                network_root: record.root.clone(),
                components_root: record.root.clone(),
                entrance_root: record.root.clone(),
                dependencies: Vec::new(),
                derived: false,
            }
        };
        for (key, value) in &record.fields {
            // A null network selects the inherited network.
            if record.base.is_some() && key == "network" && value.is_null() {
                continue;
            }
            resolved.fields.insert(key.clone(), value.clone());
            match key.as_str() {
                "network" => resolved.network_root = record.root.clone(),
                "components" => resolved.components_root = record.root.clone(),
                "entrance" => resolved.entrance_root = record.root.clone(),
                _ => {}
            }
        }
        resolved.derived = record.base.is_some();
        chain.pop();
        Ok(resolved)
    }
}
