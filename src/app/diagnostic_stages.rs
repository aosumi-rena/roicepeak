//! Exact intermediate exports from the configured voice, without another binary.
use crate::app::catalog::Catalog;
use roicepeak::{graph::Budget, manifest::Manifest, phonemes};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    time::Instant,
};

pub(super) fn render(catalog: &Catalog, args: &[OsString]) -> Result<(), String> {
    let key = args[0].to_str().ok_or("Narrator identity must be UTF-8")?;
    let voice = catalog
        .instance(key)
        .ok_or("Requested narrator is not in the resource catalog")?;
    let input = std::fs::File::open(Path::new(&args[1])).map_err(|error| error.to_string())?;
    if !input
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Token input must be a regular UTF-8 file".into());
    }
    let mut bytes = Vec::new();
    input
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 1_048_576 {
        return Err("Token input exceeds 1 MiB".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "Token input must be UTF-8")?;
    let tokens = phonemes::parse_tsv(text).map_err(|error| error.to_string())?;
    let prefix = Path::new(&args[2]);
    let name = prefix
        .file_name()
        .ok_or("Output prefix needs a file name")?;
    let parent = prefix.parent().unwrap_or(Path::new("."));
    let output = |stage: &str| {
        let mut file = name.to_os_string();
        file.push(format!("-{stage}.rmat"));
        parent.join(file)
    };
    let expected = [
        "linguistic",
        "language",
        "prosody",
        "controls",
        "pitch",
        "physical",
        "pcm",
    ];
    if expected.iter().any(|stage| output(stage).exists()) {
        return Err("A stage output already exists".into());
    }
    let manifest =
        Manifest::from_resources(&voice.binding.resources).map_err(|error| error.to_string())?;
    let mut budget = Budget {
        multiply_adds: 100_000_000_000,
        output_elements: 512 * 1024 * 1024,
    };
    let started = Instant::now();
    let mut rendered = manifest
        .render(&tokens, &mut budget)
        .map_err(|error| error.to_string())?;
    let elapsed = started.elapsed().as_secs_f64();
    let references = rendered.encoded.references;
    let nucleus = rendered.encoded.nucleus;
    rendered
        .stages
        .insert("linguistic", rendered.encoded.linguistic);
    let total = rendered
        .stages
        .values()
        .try_fold(0usize, |total, tensor| {
            total.checked_add(tensor.values().len())
        })
        .ok_or("Stage output element count overflow")?;
    if total > 64 * 1024 * 1024 {
        return Err("Combined stage output exceeds 256 MiB".into());
    }
    if rendered
        .stages
        .keys()
        .any(|stage| !expected.contains(stage))
    {
        return Err("Unexpected synthesis stage identity".into());
    }
    let mut stages = Vec::new();
    for (stage, tensor) in rendered.stages {
        let path = output(stage);
        let data = tensor.to_rmat();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        file.write_all(&data).map_err(|error| error.to_string())?;
        stages.push(
            json!({"stage":stage,"path":path,"rows":tensor.rows(),"columns":tensor.cols(),
            "bytes":data.len(),"sha256":crate::app::hex::encode(&Sha256::digest(&data))}),
        );
    }
    println!(
        "{}",
        json!({"voice":key,"token_input_sha256":crate::app::hex::encode(&Sha256::digest(&bytes)),
        "voice_metadata_sha256":crate::app::hex::encode(&Sha256::digest(voice.binding.source_bytes.as_slice())),
        "elapsed_seconds":elapsed,"stages":stages,"references":references,"nucleus":nucleus,
        "dictionary_frontend_used":false,"native_editor_boundary_applied":false,"native_parity_qualified":false})
    );
    Ok(())
}
