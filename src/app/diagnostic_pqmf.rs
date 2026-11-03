use roicepeak::{
    dnni::Node,
    graph::{Budget, Graph},
    tensor::Tensor,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    input_bits: Vec<u32>,
    taps_bits: Vec<u32>,
    #[serde(default)]
    synthesis: bool,
    #[serde(default)]
    graph: Option<GraphMode>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum GraphMode {
    Analysis,
    Synthesis,
    Roundtrip,
}

fn graph(mode: GraphMode, taps: &Tensor) -> roicepeak::Result<Graph> {
    let payloads: Vec<Vec<u8>> = (0..8)
        .map(|band| {
            let mut bytes = 65u32.to_le_bytes().to_vec();
            for value in taps.row(band) {
                bytes.extend(value.to_le_bytes());
            }
            bytes
        })
        .collect();
    let filter = |tag: &str| Node {
        tag: tag.to_owned(),
        composite: true,
        payload: &[],
        offset: 0,
        children: payloads
            .iter()
            .map(|payload| Node {
                tag: "prim1".to_owned(),
                composite: false,
                payload,
                children: Vec::new(),
                offset: 0,
            })
            .collect(),
    };
    let children = match mode {
        GraphMode::Analysis => vec![filter("_ppdsv0")],
        GraphMode::Synthesis => vec![filter("_ppusv0")],
        GraphMode::Roundtrip => vec![filter("_ppdsv0"), filter("_ppusv0")],
    };
    Graph::compile(&Node {
        tag: "modm0".to_owned(),
        composite: true,
        payload: &[],
        children,
        offset: 0,
    })
}

fn bits(tensor: &Tensor) -> Vec<u32> {
    tensor
        .values()
        .iter()
        .map(|value| value.to_bits())
        .collect()
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-pqmf accepts one JSON object on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(131_073)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 131_072 {
        return Err("PQMF diagnostic input exceeds 128 KiB".into());
    }
    let input: Input = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if input.input_bits.is_empty() || input.input_bits.len() > 8192 || input.taps_bits.len() != 520
    {
        return Err("PQMF diagnostic requires 1..8192 input words and 520 tap words".into());
    }
    if input.graph.is_some() && input.synthesis {
        return Err("Choose graph mode or direct synthesis, not both".into());
    }
    let columns = if input.synthesis || matches!(input.graph, Some(GraphMode::Synthesis)) {
        8
    } else {
        1
    };
    if input.input_bits.len() % columns != 0 {
        return Err("PQMF synthesis input must contain complete eight-channel rows".into());
    }
    let signal = Tensor::new(
        input.input_bits.len() / columns,
        columns,
        input.input_bits.into_iter().map(f32::from_bits).collect(),
    )
    .map_err(|error| error.to_string())?;
    let taps = Tensor::new(
        8,
        65,
        input.taps_bits.into_iter().map(f32::from_bits).collect(),
    )
    .map_err(|error| error.to_string())?;
    let mut budget = Budget::default();
    let output = if let Some(mode) = input.graph {
        let output = graph(mode, &taps)
            .and_then(|graph| graph.run_with(&signal, &mut budget))
            .map_err(|error| error.to_string())?;
        json!({"graph": mode, "rows": output.rows(), "columns": output.cols(), "bits": bits(&output)})
    } else if input.synthesis {
        let output = roicepeak::pqmf::synthesis(&signal, &taps, &mut budget)
            .map_err(|error| error.to_string())?;
        json!({"rows": output.rows(), "columns": output.cols(), "bits": bits(&output)})
    } else {
        let raw = roicepeak::pqmf::analysis(&signal, &taps, &mut budget)
            .map_err(|error| error.to_string())?;
        let output = roicepeak::pqmf::vocoder_analysis(&signal, &taps, &mut budget)
            .map_err(|error| error.to_string())?;
        json!({"raw_rows": raw.rows(), "raw_bits": bits(&raw), "rows": output.rows(), "columns": output.cols(), "bits": bits(&output)})
    };
    let mut bytes = serde_json::to_vec(&output).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|error| error.to_string())
}
