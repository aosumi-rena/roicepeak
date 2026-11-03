use roicepeak::{dnni, normalization::Normalizer, tensor::Tensor};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    io::{Read, Write},
};

const MAX_INPUT: usize = 4 * 1024 * 1024;
const MAX_OUTPUT: usize = 16 * 1024 * 1024;
const MAX_WORDS: usize = 1_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    cases: Vec<Case>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    Standard,
    Range,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Direction {
    Normalize,
    Denormalize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    kind: Kind,
    statistics_bits: Vec<Option<Vec<u32>>>,
    rows: usize,
    columns: usize,
    input_bits: Vec<u32>,
    direction: Direction,
    #[serde(default)]
    clip: bool,
    channels: Option<[usize; 2]>,
}

#[derive(Serialize)]
struct Sample {
    output_bits: Vec<u32>,
    source_retained_bytes: usize,
    retained_bytes: usize,
}

#[derive(Serialize)]
struct Row {
    value: Option<Sample>,
    error: Option<String>,
}

fn header(bytes: &mut Vec<u8>, tag: &[u8], children: u32, length: u32) {
    bytes.extend_from_slice(&0x7fca40ffu32.to_le_bytes());
    let mut name = [0; 8];
    name[..tag.len()].copy_from_slice(tag);
    bytes.extend_from_slice(&name);
    bytes.extend_from_slice(&children.to_le_bytes());
    bytes.extend_from_slice(&length.to_le_bytes());
}

fn node(case: &Case) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x7fca00ffu32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    let tag = match case.kind {
        Kind::Standard => b"cmpu1",
        Kind::Range => b"cmpu0",
    };
    header(&mut bytes, tag, case.statistics_bits.len() as u32, 0);
    for vector in &case.statistics_bits {
        match vector {
            None => header(&mut bytes, b"cmpno", 0, 0),
            Some(vector) => {
                header(&mut bytes, b"prim1", 0, (4 + vector.len() * 4) as u32);
                bytes.extend_from_slice(&(vector.len() as u32).to_le_bytes());
                for value in vector {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
    bytes
}

fn perform(case: Case) -> Result<Sample, String> {
    let bytes = node(&case);
    let root = dnni::parse(&bytes).map_err(|error| error.to_string())?;
    let normalizer = Normalizer::from_node(&root).map_err(|error| error.to_string())?;
    let source_retained_bytes = normalizer.estimated_bytes();
    let selected = case
        .channels
        .map(|[start, end]| normalizer.channels(start..end))
        .transpose()
        .map_err(|error| error.to_string())?;
    let normalizer = selected.as_ref().unwrap_or(&normalizer);
    let input = Tensor::new(
        case.rows,
        case.columns,
        case.input_bits.into_iter().map(f32::from_bits).collect(),
    )
    .map_err(|error| error.to_string())?;
    let output = match case.direction {
        Direction::Normalize => normalizer.normalize(&input, case.clip),
        Direction::Denormalize => normalizer.denormalize(&input),
    }
    .map_err(|error| error.to_string())?;
    Ok(Sample {
        output_bits: output
            .values()
            .iter()
            .map(|value| value.to_bits())
            .collect(),
        source_retained_bytes,
        retained_bytes: normalizer.estimated_bytes(),
    })
}

struct Output(Vec<u8>);
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > MAX_OUTPUT)
        {
            return Err(std::io::Error::other("Normalizer output exceeds 16 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-normalizer accepts JSON on stdin only".into());
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_INPUT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_INPUT {
        return Err("Normalizer input exceeds 4 MiB".into());
    }
    let input: Batch = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if input.cases.is_empty() || input.cases.len() > 64 {
        return Err("Normalizer inspection requires 1 through 64 cases".into());
    }
    let mut words = 0usize;
    for case in &input.cases {
        if case.statistics_bits.len() > 4
            || case.rows > MAX_WORDS
            || case.columns > MAX_WORDS
            || case.columns == 0
            || case.rows.checked_mul(case.columns) != Some(case.input_bits.len())
        {
            return Err("Normalizer inspection dimensions/counts exceed bounds".into());
        }
        for vector in std::iter::once(&case.input_bits)
            .chain(case.statistics_bits.iter().filter_map(Option::as_ref))
        {
            words = words
                .checked_add(vector.len())
                .filter(|count| *count <= MAX_WORDS)
                .ok_or("Normalizer aggregate input exceeds one million words")?;
            if vector
                .iter()
                .any(|value| !f32::from_bits(*value).is_finite())
            {
                return Err("Normalizer inspection requires finite input/statistics".into());
            }
        }
    }
    let mut output = Output(Vec::new());
    output
        .write_all(b"{\"cases\":[")
        .map_err(|error| error.to_string())?;
    for (index, case) in input.cases.into_iter().enumerate() {
        if index != 0 {
            output.write_all(b",").map_err(|error| error.to_string())?;
        }
        let result = match perform(case) {
            Ok(value) => Row {
                value: Some(value),
                error: None,
            },
            Err(error) => Row {
                value: None,
                error: Some(error),
            },
        };
        serde_json::to_writer(&mut output, &result).map_err(|error| error.to_string())?;
    }
    output
        .write_all(b"]}\n")
        .map_err(|error| error.to_string())?;
    std::io::stdout()
        .lock()
        .write_all(&output.0)
        .map_err(|error| error.to_string())
}
