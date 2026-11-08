//! Explicit headless typed native-project round trip; never loads preferences.
use crate::app::{
    catalog::Catalog,
    document::{ControlValue, Parameters, Project},
    native_interop,
    native_project::NativeProject,
    project_files,
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
    path::Path,
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

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !(args.len() == 3 || args.len() == 4)
        || args.iter().any(|a| {
            a.to_str()
                .is_none_or(|text| text.is_empty() || text.encode_utf16().count() > 32768)
        })
    {
        return Err("Usage: roicepeak --inspect-native-editable LOCATIONS.json INPUT.vpp EXISTING_OUTPUT_DIRECTORY [EDIT.json]".into());
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
    let names: &[&str] = if args.len() == 4 {
        &["roundtrip.vpp", "edited.vpp", "report.json"]
    } else {
        &["roundtrip.vpp", "report.json"]
    };
    for name in names {
        let path = directory.join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => return Err(format!("Output already exists: {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    let config_bytes = bounded(config_path, 65536)?;
    let input = bounded(input_path, DOCUMENT_LIMIT)?;
    let edit_bytes = (args.len() == 4)
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
    let original_root = NativeProject::parse(&input)?.root().clone();
    let project = project_files::read(input_path, Some(&config))?;
    if !origin_matches(&project, &input, &original_root) {
        return Err("Typed decode did not preserve the native origin".into());
    }
    controls_budget(&project)?;
    let initial_bits = controls(&project);
    let encoded = native_interop::encode(&project, Some(&config))?;
    if encoded != input {
        return Err("Typed no-op native encode changed original bytes".into());
    }
    // Validate every requested edit and stage its native output before first write.
    let optional = edit
        .as_ref()
        .map(|edit| {
            let (project, edit_report) = edited(&project, edit)?;
            controls_budget(&project)?;
            let expected_controls = controls(&project);
            let native = native_interop::encode(&project, Some(&config))?;
            Ok::<_, String>((expected_controls, native, edit_report))
        })
        .transpose()?;
    let check_inputs = || -> Result<(), String> {
        if bounded(config_path, 65536)? != config_bytes
            || bounded(input_path, DOCUMENT_LIMIT)? != input
        {
            return Err("An explicit diagnostic input changed during round trip".into());
        }
        if let Some(bytes) = &edit_bytes {
            if bounded(Path::new(&args[3]), 4096)? != *bytes {
                return Err("Edit request changed during round trip".into());
            }
        }
        Ok(())
    };
    check_inputs()?;
    // New outputs never overwrite existing files, including after an early failure.
    let roundtrip_path = directory.join("roundtrip.vpp");
    write_new(&roundtrip_path, &encoded)?;
    let reopened = project_files::read(&roundtrip_path, Some(&config))?;
    controls_budget(&reopened)?;
    let reopened_bits = controls(&reopened);
    if reopened_bits != initial_bits
        || !origin_matches(&reopened, &input, &original_root)
        || native_interop::encode(&reopened, Some(&config))? != input
    {
        return Err("Native reopen changed controls, original fields or bytes".into());
    }
    let mut artifacts =
        vec![json!({"name":"roundtrip.vpp","bytes":encoded.len(),"sha256":digest(&encoded)})];
    let edit_report = if let Some((expected_controls, native, edit_report)) = optional {
        let path = directory.join("edited.vpp");
        write_new(&path, &native)?;
        let reopened = project_files::read(&path, Some(&config))?;
        controls_budget(&reopened)?;
        let reopened_controls = controls(&reopened);
        if reopened_controls != expected_controls
            || native_interop::encode(&reopened, Some(&config))? != native
        {
            return Err("Edited native reopen changed controls or bytes".into());
        }
        artifacts.push(json!({"name":"edited.vpp","bytes":native.len(),"sha256":digest(&native)}));
        Some(json!({"edit":edit_report,"controls":reopened_controls,
            "native_encode_sha256":digest(&native)}))
    } else {
        None
    };
    check_inputs()?;
    let report = json!({"schema":"roicepeak.native-editable-diagnostic.v2",
        "input":{"path":input_path,"bytes":input.len(),"sha256":digest(&input)},
        "locations":{"path":config_path,"sha256":digest(&config_bytes),
            "allow_out_of_range":config.value_policy.allow_out_of_range,
            "allow_negative":config.value_policy.allow_negative},
        "edit_input":edit_bytes.as_ref().map(|bytes|json!({"bytes":bytes.len(),"sha256":digest(bytes)})),
        "catalog":{"voices":catalog.voices.len(),"problems":catalog.problems},
        "before_controls":initial_bits,"reopened_controls":reopened_bits,
        "no_op_native_bytes_equal":true,"native_reopen_equal":true,
        "edited":edit_report,"artifacts":artifacts,
        "scope":{"typed_roundtrip":true,"synthesis_executed":false,"native_editor_used":false,
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
        "no_op_native_bytes_equal":true,"native_reopen_equal":true,"native_editor_acceptance_qualified":false})
    );
    Ok(())
}
