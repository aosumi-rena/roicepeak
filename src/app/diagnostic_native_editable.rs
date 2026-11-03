//! Explicit headless typed native-project round trip; never loads preferences.
use crate::app::{
    catalog::Catalog,
    document::{ControlValue, Parameters, Project},
    native_interop,
    native_project::NativeProject,
    render::Config,
    resources::Locations,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const DOCUMENT_LIMIT: usize = 16 * 1024 * 1024;
const REPORT_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Scalar {
    Speed,
    Pitch,
    Pause,
    Volume,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    field: Scalar,
    value_bits: u32,
}

fn digest(bytes: &[u8]) -> String {
    crate::app::hex::encode(&Sha256::digest(bytes))
}

fn bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Diagnostic inputs must be regular files".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err(format!("{} exceeds {limit} bytes", path.display()));
    }
    Ok(bytes)
}

fn controls_budget(project: &Project) -> Result<(), String> {
    let (mut words, mut label_bytes) = (0usize, 0usize);
    for parameters in std::iter::once(&project.global)
        .chain(project.narrator_settings.values())
        .chain(project.blocks.iter().map(|block| &block.parameters))
    {
        words = words
            .checked_add(4 + parameters.emotions.len())
            .ok_or("Control-report word count overflow")?;
        if words > 32768 {
            return Err("Control report exceeds 32768 words".into());
        }
        for key in parameters.emotions.keys() {
            label_bytes = label_bytes
                .checked_add(key.len())
                .ok_or("Control-report label count overflow")?;
            if label_bytes > 1_048_576 {
                return Err("Control-report labels exceed 1 MiB".into());
            }
        }
    }
    Ok(())
}

fn control_bits(parameters: &Parameters) -> Value {
    json!({"scalars":{"speed":parameters.speed.bits(),"pitch":parameters.pitch.bits(),
        "pause":parameters.pause.bits(),"volume":parameters.volume.bits()},
        "emotions":parameters.emotions.iter().map(|(key,value)|
            json!({"key":key,"bits":value.bits()})).collect::<Vec<_>>()})
}

fn controls(project: &Project) -> Value {
    json!({"global":control_bits(&project.global),
        "narrators":project.narrator_settings.iter().map(|(identity,parameters)|
            json!({"identity":identity,"parameters":control_bits(parameters)})).collect::<Vec<_>>(),
        "blocks":project.blocks.iter().map(|block|json!({"id":block.id,"voice":block.narrator,
            "parameters":control_bits(&block.parameters)})).collect::<Vec<_>>()})
}

fn own_bytes(project: &Project) -> Result<Vec<u8>, String> {
    project.validate()?;
    let bytes = serde_json::to_vec_pretty(project).map_err(|e| e.to_string())?;
    if bytes.len() > DOCUMENT_LIMIT {
        return Err("Own project exceeds 16 MiB".into());
    }
    Ok(bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    drop(file);
    if bounded(path, bytes.len())? != bytes {
        return Err(format!("Output readback differs: {}", path.display()));
    }
    Ok(())
}

fn origin_matches(project: &Project, input: &[u8], root: &Value) -> bool {
    project
        .native_origin
        .as_ref()
        .is_some_and(|origin| origin.original_bytes() == input && origin.root() == root)
}

fn edited(project: &Project, edit: &Edit) -> Result<(Project, Value), String> {
    let value = ControlValue::from_normalized(f32::from_bits(edit.value_bits));
    if !value.displayable() {
        return Err("Edited scalar must be finite and displayable".into());
    }
    let mut next = project.clone();
    let (name, slot) = match edit.field {
        Scalar::Speed => ("speed", &mut next.global.speed),
        Scalar::Pitch => ("pitch", &mut next.global.pitch),
        Scalar::Pause => ("pause", &mut next.global.pause),
        Scalar::Volume => ("volume", &mut next.global.volume),
    };
    let before = slot.bits();
    *slot = value;
    next.validate()?;
    Ok((
        next,
        json!({"scope":"global","field":name,"before_bits":before,
        "requested_bits":edit.value_bits,"after_bits":value.bits(),
        "normalized_value":value.normalized()}),
    ))
}

fn same_parameters(a: &Parameters, b: &Parameters) -> bool {
    [
        a.speed.bits(),
        a.pitch.bits(),
        a.pause.bits(),
        a.volume.bits(),
    ] == [
        b.speed.bits(),
        b.pitch.bits(),
        b.pause.bits(),
        b.volume.bits(),
    ] && a.emotions.keys().chain(b.emotions.keys()).all(|key| {
        a.emotions
            .get(key)
            .copied()
            .unwrap_or(ControlValue::ZERO)
            .bits()
            == b.emotions
                .get(key)
                .copied()
                .unwrap_or(ControlValue::ZERO)
                .bits()
    })
}

fn fresh_roundtrip(
    config: &Config,
    config_path: &Path,
    config_bytes: &[u8],
    input_path: &Path,
    input: &[u8],
    directory: &Path,
) -> Result<(), String> {
    let project: Project = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    project.validate()?;
    if project.native_origin.is_some() {
        return Err("--new-project requires an own project without a native origin".into());
    }
    controls_budget(&project)?;
    let own = own_bytes(&project)?;
    let encoded = native_interop::encode(&project, Some(config))?;
    let native = NativeProject::parse(&encoded)?;
    let root = native.root().clone();
    let decoded = native_interop::decode(native, Some(config))?;
    controls_budget(&decoded)?;
    let roundtrip = native_interop::encode(&decoded, Some(config))?;
    if roundtrip != encoded
        || !same_parameters(&project.global, &decoded.global)
        || project.blocks.len() != decoded.blocks.len()
    {
        return Err("New native round trip changed its stored controls or structure".into());
    }
    for identity in project
        .narrator_settings
        .keys()
        .chain(decoded.narrator_settings.keys())
    {
        let neutral = Parameters::default();
        if !same_parameters(
            project.narrator_settings.get(identity).unwrap_or(&neutral),
            decoded.narrator_settings.get(identity).unwrap_or(&neutral),
        ) {
            return Err("New native round trip changed narrator controls".into());
        }
    }
    let mut export = project.export_settings.clone();
    export.name = decoded.export_settings.name.clone();
    if export != decoded.export_settings {
        return Err("New native round trip changed stored export settings".into());
    }
    let mut expected_project = project.clone();
    expected_project.synchronize_sentences()?;
    for (before, after) in expected_project.blocks.iter().zip(&decoded.blocks) {
        if before.text != after.text
            || before.narrator != after.narrator
            || before.offset_mode != after.offset_mode
            || before.offset_seconds.to_bits() != after.offset_seconds.to_bits()
            || !same_parameters(&before.parameters, &after.parameters)
        {
            return Err("New native round trip changed a block".into());
        }
        if before.text.trim().is_empty() {
            let partition = crate::app::sentences::partition(&before.text);
            if partition.len() != after.sentences.len()
                || partition
                    .iter()
                    .zip(&after.sentences)
                    .any(|(range, sentence)| {
                        before.text.get(range.bytes.clone()) != Some(sentence.text.as_str())
                            || !sentence.words.is_empty()
                    })
            {
                return Err("Whitespace-only text acquired native words".into());
            }
            continue;
        }
        if before.sentences.len() != after.sentences.len() {
            return Err("New native round trip changed sentence boundaries".into());
        }
        for (before, after) in before.sentences.iter().zip(&after.sentences) {
            let expected = before
                .words
                .iter()
                .enumerate()
                .filter_map(|(index, word)| {
                    if matches!(word.pos, 0x2000 | 0x2001) {
                        return None;
                    }
                    let mut word = word.clone();
                    for (mora, baseline) in word
                        .morae
                        .iter_mut()
                        .zip(&before.query.predicted[index].morae)
                    {
                        if !mora.accent_edited {
                            mora.code = baseline.code;
                        }
                        mora.prediction_eligible = baseline.prediction_eligible;
                    }
                    Some(word)
                })
                .collect::<Vec<_>>();
            if before.text != after.text
                || serde_json::to_vec(&expected).map_err(|e| e.to_string())?
                    != serde_json::to_vec(&after.words).map_err(|e| e.to_string())?
            {
                return Err("New native round trip changed stored word controls".into());
            }
        }
    }
    let reopened_own = own_bytes(&decoded)?;
    let report = json!({"operation":"new-native-project-roundtrip","input":input_path,
        "input_sha256":digest(input),"native_origin_was_present":false,
        "native_encode_sha256":digest(&encoded),"native_roundtrip_exact":true,
        "source_controls":controls(&project),"decoded_controls":controls(&decoded),"document":root,
        "native_editor_acceptance_qualified":false});
    let report = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    if report.len() > REPORT_LIMIT {
        return Err("New native report exceeds its byte budget".into());
    }
    if bounded(config_path, 65536)? != config_bytes || bounded(input_path, DOCUMENT_LIMIT)? != input
    {
        return Err("An explicit new-project diagnostic input changed before output".into());
    }
    for (name, bytes) in [
        ("source.roicepeak", own.as_slice()),
        ("fresh.vpp", encoded.as_slice()),
        ("reopened.roicepeak", reopened_own.as_slice()),
        ("roundtrip.vpp", roundtrip.as_slice()),
        ("report.json", report.as_slice()),
    ] {
        write_new(&directory.join(name), bytes)?;
    }
    if bounded(config_path, 65536)? != config_bytes || bounded(input_path, DOCUMENT_LIMIT)? != input
    {
        return Err("An explicit new-project diagnostic input changed during output".into());
    }
    println!(
        "{}",
        json!({"operation":"new-native-project-roundtrip","native_encode_sha256":digest(&encoded),
        "blocks":decoded.blocks.len(),"native_origin_was_present":false,"native_roundtrip_exact":true,
        "native_editor_acceptance_qualified":false})
    );
    Ok(())
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    let prepared_mode = args.len() == 5 && args[3] == "--prepared-project";
    let fresh_mode = args.len() == 4 && args[3] == "--new-project";
    if !(args.len() == 3 || args.len() == 4 || prepared_mode)
        || args.iter().any(|a| {
            a.to_str()
                .is_none_or(|text| text.is_empty() || text.encode_utf16().count() > 32768)
        })
    {
        return Err("Usage: roicepeak --inspect-native-editable LOCATIONS.json INPUT EXISTING_OUTPUT_DIRECTORY [EDIT.json | --prepared-project PREPARED.roicepeak | --new-project]".into());
    }
    let mut gate = Vec::new();
    std::io::stdin()
        .lock()
        .take(9)
        .read_to_end(&mut gate)
        .map_err(|error| error.to_string())?;
    if gate != b"RPNAT12\n" {
        return Err(
            "--inspect-native-editable requires the eight-byte diagnostic stdin gate".into(),
        );
    }
    let config_path = Path::new(&args[0]);
    let input_path = Path::new(&args[1]);
    let directory = fs::canonicalize(Path::new(&args[2])).map_err(|e| e.to_string())?;
    if !directory.is_dir() {
        return Err("Output must be an existing directory".into());
    }
    let names: Vec<&str> = if fresh_mode {
        vec![
            "source.roicepeak",
            "fresh.vpp",
            "reopened.roicepeak",
            "roundtrip.vpp",
            "report.json",
        ]
    } else if args.len() >= 4 {
        vec![
            "roundtrip.roicepeak",
            "roundtrip.vpp",
            "edited.roicepeak",
            "edited.vpp",
            "report.json",
        ]
    } else {
        vec!["roundtrip.roicepeak", "roundtrip.vpp", "report.json"]
    };
    let paths: Vec<PathBuf> = names.iter().map(|name| directory.join(name)).collect();
    // Fixed child names cannot traverse the supplied directory. create_new below is
    // authoritative if another process creates a file after this early check.
    for path in &paths {
        match fs::symlink_metadata(path) {
            Ok(_) => return Err(format!("Output already exists: {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    let config_bytes = bounded(config_path, 65536)?;
    let input = bounded(input_path, DOCUMENT_LIMIT)?;
    let prepared_path = prepared_mode.then(|| Path::new(&args[4]));
    let prepared_bytes = prepared_path
        .map(|path| bounded(path, DOCUMENT_LIMIT))
        .transpose()?;
    let edit_bytes = (args.len() == 4 && !fresh_mode)
        .then(|| bounded(Path::new(&args[3]), 4096))
        .transpose()?;
    let edit: Option<Edit> = edit_bytes
        .as_deref()
        .map(serde_json::from_slice)
        .transpose()
        .map_err(|e| e.to_string())?;
    let locations = Locations::read(config_path)?;
    let catalog = Catalog::load_roots(&locations.voice_roots)?;
    let mut config = Config::from_catalog(
        locations
            .dictionary
            .clone()
            .ok_or("Explicit locations require a dictionary path")?,
        &catalog,
    )?;
    config.value_policy = locations.value_policy();
    if fresh_mode {
        return fresh_roundtrip(
            &config,
            config_path,
            &config_bytes,
            input_path,
            &input,
            &directory,
        );
    }
    let native = NativeProject::parse(&input)?;
    let original_root = native.root().clone();
    let project = native_interop::decode(native, Some(&config))?;
    if project.format != 12 || !origin_matches(&project, &input, &original_root) {
        return Err("Typed decode did not preserve format12/native origin".into());
    }
    controls_budget(&project)?;
    let initial_bits = controls(&project);
    let encoded = native_interop::encode(&project, Some(&config))?;
    if encoded != input {
        return Err("Typed no-op native encode changed original bytes".into());
    }
    let own = own_bytes(&project)?;
    // Validate every requested edit and stage its native output before first write.
    let optional = if let Some(path) = prepared_path {
        let prepared = Project::read(path)?;
        controls_budget(&prepared)?;
        if !origin_matches(&prepared, &input, &original_root) {
            return Err("Prepared project does not belong to the supplied native input".into());
        }
        let own = own_bytes(&prepared)?;
        // Production Save performs the accepted-query, lexical-topology and range checks.
        let native = native_interop::encode(&prepared, Some(&config))?;
        Some((
            prepared,
            own,
            native,
            json!({"scope":"prepared-own-project"}),
        ))
    } else {
        edit.as_ref()
            .map(|edit| {
                let (project, edit_report) = edited(&project, edit)?;
                let own = own_bytes(&project)?;
                let native = native_interop::encode(&project, Some(&config))?;
                Ok::<_, String>((project, own, native, edit_report))
            })
            .transpose()?
    };
    if bounded(config_path, 65536)? != config_bytes || bounded(input_path, DOCUMENT_LIMIT)? != input
    {
        return Err("An explicit diagnostic input changed before output".into());
    }
    if let Some(bytes) = &edit_bytes {
        if bounded(Path::new(&args[3]), 4096)? != *bytes {
            return Err("Edit request changed before output".into());
        }
    }
    if let Some((path, bytes)) = prepared_path.zip(prepared_bytes.as_ref()) {
        if bounded(path, DOCUMENT_LIMIT)? != *bytes {
            return Err("Prepared project changed before output".into());
        }
    }
    // Same validated own serialization as Project::write, guarded by create_new.
    // An unsuccessful invocation retains only its own new partial outputs; it
    // never removes files or retries/overwrites an existing artifact.
    write_new(&directory.join("roundtrip.roicepeak"), &own)?;
    let reopened = Project::read(&directory.join("roundtrip.roicepeak"))?;
    controls_budget(&reopened)?;
    let reopened_bits = controls(&reopened);
    if reopened != project
        || reopened_bits != initial_bits
        || !origin_matches(&reopened, &input, &original_root)
    {
        return Err("Own serialization/reopen changed canonical state or native origin".into());
    }
    let roundtrip = native_interop::encode(&reopened, Some(&config))?;
    if roundtrip != input {
        return Err("Own reopen changed native no-op bytes".into());
    }
    write_new(&directory.join("roundtrip.vpp"), &roundtrip)?;
    let mut artifacts = vec![
        json!({"name":"roundtrip.roicepeak","bytes":own.len(),"sha256":digest(&own)}),
        json!({"name":"roundtrip.vpp","bytes":roundtrip.len(),"sha256":digest(&roundtrip)}),
    ];
    let edit_report = if let Some((expected, own, native, edit_report)) = optional {
        write_new(&directory.join("edited.roicepeak"), &own)?;
        let reopened = Project::read(&directory.join("edited.roicepeak"))?;
        controls_budget(&reopened)?;
        if reopened != expected
            || !origin_matches(&reopened, &input, &original_root)
            || native_interop::encode(&reopened, Some(&config))? != native
        {
            return Err("Edited own reopen changed canonical state, origin or native bytes".into());
        }
        write_new(&directory.join("edited.vpp"), &native)?;
        artifacts.push(json!({"name":"edited.roicepeak","bytes":own.len(),"sha256":digest(&own)}));
        artifacts.push(json!({"name":"edited.vpp","bytes":native.len(),"sha256":digest(&native)}));
        Some(json!({"edit":edit_report,"controls":controls(&reopened),
            "origin_unchanged":true,"native_encode_sha256":digest(&native)}))
    } else {
        None
    };
    if bounded(config_path, 65536)? != config_bytes || bounded(input_path, DOCUMENT_LIMIT)? != input
    {
        return Err("Explicit diagnostic inputs changed during round trip".into());
    }
    if let Some(bytes) = &edit_bytes {
        if bounded(Path::new(&args[3]), 4096)? != *bytes {
            return Err("Edit request changed during round trip".into());
        }
    }
    if let Some((path, bytes)) = prepared_path.zip(prepared_bytes.as_ref()) {
        if bounded(path, DOCUMENT_LIMIT)? != *bytes {
            return Err("Prepared project changed during round trip".into());
        }
    }
    let report = json!({"schema":"roicepeak.native-editable-diagnostic.v1","project_format":12,
        "input":{"path":input_path,"bytes":input.len(),"sha256":digest(&input)},
        "locations":{"path":config_path,"sha256":digest(&config_bytes),
            "allow_out_of_range":config.value_policy.allow_out_of_range,
            "allow_negative":config.value_policy.allow_negative},
        "edit_input":edit_bytes.as_ref().map(|bytes|json!({"bytes":bytes.len(),"sha256":digest(bytes)})),
        "prepared_input":prepared_bytes.as_ref().map(|bytes|json!({"path":prepared_path,
            "bytes":bytes.len(),"sha256":digest(bytes)})),
        "catalog":{"voices":catalog.voices.len(),"problems":catalog.problems},
        "before_controls":initial_bits,"reopened_controls":reopened_bits,
        "no_op_native_bytes_equal":true,"own_reopen_equal":true,"origin_unchanged":true,
        "edited":edit_report,"artifacts":artifacts,
        "scope":{"typed_roundtrip":true,"prepared_project_loaded":prepared_mode,
            "synthesis_executed":false,"native_editor_used":false,
            "native_editor_acceptance_qualified":false,"unicode_or_structural_projection_qualified":false,
            "preferences_loaded_or_saved":false,"user_dictionary_loaded":false}});
    let bytes = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    if bytes.len() > REPORT_LIMIT {
        return Err("Diagnostic report exceeds 16 MiB".into());
    }
    write_new(&directory.join("report.json"), &bytes)?;
    println!(
        "{}",
        json!({"report":directory.join("report.json"),"sha256":digest(&bytes),
        "no_op_native_bytes_equal":true,"own_reopen_equal":true,"native_editor_acceptance_qualified":false})
    );
    Ok(())
}
