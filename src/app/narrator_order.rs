//! Optional application-local UI collection order, independent of model/default order.
use crate::app::{
    catalog::Catalog,
    document::{VoiceRef, VoiceVersion},
};
use std::{collections::BTreeMap, io::Read, sync::OnceLock};

#[derive(Default)]
struct Order {
    ranks: BTreeMap<String, usize>,
}
static ORDER: OnceLock<Result<Order, String>> = OnceLock::new();

fn load() -> Result<Order, String> {
    let path = std::env::current_exe()
        .map_err(|error| error.to_string())?
        .parent()
        .ok_or("Application directory unavailable")?
        .join("resources/narrator_order.json");
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Order::default()),
        Err(error) => return Err(format!("Narrator UI order: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 65_536 {
        return Err("Narrator UI order exceeds 64 KiB".into());
    }
    let names: Vec<String> =
        serde_json::from_slice(&bytes).map_err(|error| format!("Narrator UI order: {error}"))?;
    if names.len() > 512
        || names
            .iter()
            .any(|name| name.is_empty() || name.len() > 4096 || name.contains('\0'))
    {
        return Err("Invalid narrator UI collection order".into());
    }
    let mut ranks = BTreeMap::new();
    for (index, name) in names.into_iter().enumerate() {
        // Use the first matching collection name.
        ranks.entry(name).or_insert(index);
    }
    Ok(Order { ranks })
}

pub(crate) fn indices(catalog: &Catalog) -> Result<Vec<usize>, String> {
    let order = ORDER.get_or_init(load).as_ref().map_err(Clone::clone)?;
    let mut indices: Vec<_> = (0..catalog.voices.len()).collect();
    // Retain discovery order for equal collection positions.
    indices.sort_by_key(|&index| {
        let voice = &catalog.voices[index];
        (
            voice.can_commercial,
            order
                .ranks
                .get(&voice.collection_name)
                .copied()
                .unwrap_or(1000),
        )
    });
    Ok(indices)
}

/// One character entry per narrator identity; source versions remain separate bindings.
pub(crate) fn identity_indices(catalog: &Catalog) -> Result<Vec<usize>, String> {
    let mut positions: BTreeMap<String, usize> = BTreeMap::new();
    let mut identities: Vec<usize> = Vec::new();
    for index in indices(catalog)? {
        let voice = &catalog.voices[index];
        if let Some(&position) = positions.get(&voice.name) {
            if voice.version > catalog.voices[identities[position]].version {
                identities[position] = index;
            }
        } else {
            positions.insert(voice.name.clone(), identities.len());
            identities.push(index);
        }
    }
    Ok(identities)
}

/// The desktop frontend currently prepares Japanese; language is not inferred from a bank.
pub(crate) fn resolve_choice(
    catalog: &Catalog,
    current: &VoiceRef,
    choice_key: &str,
) -> Result<VoiceRef, String> {
    let choice = catalog
        .instance(choice_key)
        .ok_or("Selected narrator is unavailable")?;
    if current.identity == choice.name {
        return Ok(current.clone());
    }
    let mut reference = VoiceRef {
        identity: choice.name.clone(),
        language: current.language.clone(),
        version: current.version.clone(),
    };
    if catalog.voice(&reference).is_none() {
        reference.version = VoiceVersion::Latest;
    }
    Ok(reference)
}
