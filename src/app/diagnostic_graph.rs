//! Explicit bounded graph inference with production DNNI and RMAT codecs.
use roicepeak::{
    dnni,
    graph::{Budget, Graph},
    tensor::Tensor,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
};

const MODEL_BYTES: usize = 1024 * 1024;
const ELEMENTS: usize = 256 * 1024;
const RMAT_BYTES: usize = 12 + ELEMENTS * 4;
const NODE_LIMIT: usize = 4096;

fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err("Graph input must be a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err("Graph input grew beyond its byte budget".into());
    }
    Ok(bytes)
}

fn nodes(node: &dnni::Node<'_>, count: &mut usize) -> Result<(), String> {
    *count += 1;
    if *count > NODE_LIMIT {
        return Err("Graph diagnostic node budget exceeded".into());
    }
    for child in &node.children {
        nodes(child, count)?;
    }
    Ok(())
}

fn default_activation() -> (&'static str, bool, bool) {
    #[cfg(target_arch = "x86_64")]
    {
        let avx = std::arch::is_x86_feature_detected!("avx");
        let fma = std::arch::is_x86_feature_detected!("fma");
        (if avx && fma { "fused" } else { "separate" }, avx, fma)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        ("unsupported-generated-backend", false, false)
    }
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if args.len() != 3 {
        return Err("Usage: --inspect-graph MODEL INPUT.rmat NEW_OUTPUT.rmat".into());
    }
    // The bounded controller assigns its Job before providing this fixed barrier.
    let mut barrier = Vec::new();
    std::io::stdin()
        .lock()
        .take(9)
        .read_to_end(&mut barrier)
        .map_err(|error| error.to_string())?;
    if barrier != b"RGRAPH1\0" {
        return Err("Graph diagnostic requires the fixed controller stdin barrier".into());
    }
    let output_path = Path::new(&args[2]);
    if output_path.exists() {
        return Err("Graph output already exists".into());
    }
    let model_bytes = read(Path::new(&args[0]), MODEL_BYTES)?;
    let input_bytes = read(Path::new(&args[1]), RMAT_BYTES)?;
    let input = Tensor::from_rmat(&input_bytes).map_err(|error| error.to_string())?;
    if input.rows() > 256 || input.cols() > 4096 || input.values().len() > ELEMENTS {
        return Err("Graph diagnostic tensor dimensions exceed bounds".into());
    }
    let root = dnni::parse(&model_bytes).map_err(|error| error.to_string())?;
    let mut node_count = 0;
    nodes(&root, &mut node_count)?;
    let graph = Graph::compile(&root).map_err(|error| error.to_string())?;
    let mut budget = Budget {
        multiply_adds: 8_000_000,
        output_elements: ELEMENTS,
    };
    let output = graph
        .run_with(&input, &mut budget)
        .map_err(|error| error.to_string())?;
    if output.values().len() > ELEMENTS {
        return Err("Graph output exceeds diagnostic element budget".into());
    }
    output.ensure_finite().map_err(|error| error.to_string())?;
    let bytes = output.to_rmat();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    let (arithmetic, avx, fma) = default_activation();
    println!(
        "{}",
        json!({"model_sha256":crate::app::hex::encode(&Sha256::digest(&model_bytes)),
            "input_sha256":crate::app::hex::encode(&Sha256::digest(&input_bytes)),
            "output_sha256":crate::app::hex::encode(&Sha256::digest(&bytes)),
            "output_path":output_path,"output_bytes":bytes.len(),
            "root_tag":root.tag,"node_count":node_count,
            "input_rows":input.rows(),"input_columns":input.cols(),
            "rows":output.rows(),"columns":output.cols(),"elements":output.values().len(),
            "retained_graph_bytes":graph.estimated_bytes(),
            "remaining_multiply_adds":budget.multiply_adds,
            "remaining_output_elements":budget.output_elements,
            "default_activation_arithmetic":arithmetic,"avx":avx,"fma":fma,
            "native_parity_qualified":false})
    );
    Ok(())
}
