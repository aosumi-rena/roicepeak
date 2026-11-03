use roicepeak::{dnni, graph::Budget, tensor::Tensor, vocoder};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    time::Instant,
};

const STAGES: [&str; 15] = [
    "normalized",
    "shape",
    "hidden",
    "source",
    "harmonic-parameters",
    "harmonic",
    "subbands",
    "noise",
    "upsampled",
    "bands",
    "gated",
    "aperiodic-source",
    "noise-parameters",
    "aperiodic",
    "pcm",
];

fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Vocoder input must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err("Vocoder input byte budget".into());
    }
    Ok(bytes)
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if args.len() != 3 {
        return Err("Usage: --inspect-vocoder-stages MODEL PHYSICAL.rmat NEW_PREFIX".into());
    }
    let prefix = Path::new(&args[2]);
    let name = prefix
        .file_name()
        .ok_or("Stage prefix requires a file name")?;
    let parent = prefix.parent().unwrap_or(Path::new("."));
    let output = |stage: &str| {
        let mut name = name.to_os_string();
        name.push(format!("-{stage}.rmat"));
        parent.join(name)
    };
    if STAGES.iter().any(|stage| output(stage).exists()) {
        return Err("A vocoder stage output already exists".into());
    }
    let model_bytes = read(Path::new(&args[0]), dnni::MAX_BYTES)?;
    let physical_bytes = read(Path::new(&args[1]), 4096 * 72 * 4 + 12)?;
    let physical = Tensor::from_rmat(&physical_bytes).map_err(|error| error.to_string())?;
    if physical.cols() != 72 || !(1..=4096).contains(&physical.rows()) {
        return Err("Vocoder diagnostic requires 1..4096 physical frames of 72 channels".into());
    }
    let root = dnni::parse(&model_bytes).map_err(|error| error.to_string())?;
    let model = vocoder::Vocoder::compile(&root).map_err(|error| error.to_string())?;
    let log_gain = f32::from_le_bytes(
        root.payload
            .get(4..8)
            .ok_or("Vocoder gain field")?
            .try_into()
            .map_err(|_| "Vocoder gain width")?,
    );
    let window = vocoder::diagnostic_lf_window(log_gain, &[0.0; 480], &[0.0; 480], None)
        .map_err(|error| error.to_string())?;
    let mut budget = Budget {
        multiply_adds: 20_000_000_000,
        output_elements: 128 * 1024 * 1024,
    };
    let started = Instant::now();
    let rendered = model
        .run(&physical, &mut budget)
        .map_err(|error| error.to_string())?;
    let elapsed = started.elapsed().as_secs_f64();
    if rendered.stages.len() != STAGES.len()
        || rendered.stages.keys().any(|stage| !STAGES.contains(stage))
    {
        return Err("Unexpected vocoder stage identity".into());
    }
    let elements = rendered
        .stages
        .values()
        .try_fold(0usize, |sum, tensor| sum.checked_add(tensor.values().len()))
        .ok_or("Vocoder stage element overflow")?;
    if elements > 64 * 1024 * 1024 {
        return Err("Vocoder stage output exceeds 256 MiB".into());
    }
    let mut stages = Vec::new();
    for (stage, tensor) in rendered.stages {
        let path = output(stage);
        let bytes = tensor.to_rmat();
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        stages.push(
            json!({"stage":stage,"path":path,"rows":tensor.rows(),"columns":tensor.cols(),
            "bytes":bytes.len(),"sha256":crate::app::hex::encode(&Sha256::digest(&bytes))}),
        );
    }
    println!(
        "{}",
        json!({"model_sha256":crate::app::hex::encode(&Sha256::digest(&model_bytes)),
        "physical_sha256":crate::app::hex::encode(&Sha256::digest(&physical_bytes)),
        "physical_frames":physical.rows(),"excitation_log_gain_bits":log_gain.to_bits(),
        "excitation_gain_bits":window.gain.to_bits(),"fused":window.fused,
        "elapsed_seconds":elapsed,"stages":stages,"native_parity_qualified":false})
    );
    Ok(())
}
