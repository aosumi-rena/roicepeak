//! Explicit headless access to the same export backend used by the editor.
use crate::app::{catalog::Catalog, document::Project, export, render, resources::Locations};
use serde::Deserialize;
use std::{collections::BTreeMap, ffi::OsString, io::Read, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    settings: export::Settings,
    #[serde(default)]
    scope: export::Scope,
    #[serde(default)]
    voice_names: BTreeMap<String, String>,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    let (dictionary_path, args) = if args.first().is_some_and(|arg| arg == "--user-dictionary") {
        (
            Some(args.get(1).ok_or("--user-dictionary requires a file")?),
            &args[2..],
        )
    } else {
        (None, args)
    };
    if !(args.len() == 4 || (args.len() == 5 && args[4] == "--overwrite")) {
        return Err("Usage: roicepeak --export-project [--user-dictionary FILE] LOCATIONS.json PROJECT.roicepeak EXPORT.json OUTPUT_DIRECTORY [--overwrite]".into());
    }
    let user_dictionary = match dictionary_path {
        Some(path) => crate::app::dictionary::Snapshot::read(Path::new(path))?,
        None => crate::app::dictionary::Snapshot::empty(),
    };
    let locations = Locations::read(Path::new(&args[0]))?;
    let project = Project::read(Path::new(&args[1]))?;
    let input = std::fs::File::open(Path::new(&args[2])).map_err(|error| error.to_string())?;
    if !input
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Export settings must be a regular JSON file".into());
    }
    let mut bytes = Vec::new();
    input
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("Export settings exceed 1 MiB".into());
    }
    let options: Options = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if options.voice_names.len() > 256
        || options.voice_names.iter().any(|(key, name)| {
            key.is_empty() || key.len() > 4096 || name.is_empty() || name.len() > 4096
        })
    {
        return Err("Invalid export voice display names".into());
    }
    let catalog = Catalog::load_roots(&locations.voice_roots)?;
    let mut names: BTreeMap<String, String> = catalog
        .voices
        .iter()
        .map(|voice| (voice.name.clone(), voice.name.clone()))
        .collect();
    for (key, name) in options.voice_names {
        if !names.contains_key(&key) {
            return Err("Export display name refers to an unavailable voice".into());
        }
        names.insert(key, name);
    }
    let policy = locations.value_policy();
    let mut config = render::Config::from_catalog(
        locations
            .dictionary
            .ok_or("No pronunciation dictionary is available")?,
        &catalog,
    )?;
    config.value_policy = policy;
    let files = export::write_to_directory(
        render::Request {
            config,
            project,
            user_dictionary,
        },
        options.scope,
        options.settings,
        names,
        Path::new(&args[3]),
        args.len() == 5,
    )?;
    println!(
        "{}",
        serde_json::json!({"files":files,"native_parity_qualified":false})
    );
    Ok(())
}
