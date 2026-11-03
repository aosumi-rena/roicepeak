use roicepeak::vocoder::{Effects, SpectrumKind};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    periodic: Vec<[f32; 2]>,
    aperiodic: Vec<[f32; 2]>,
    packed: Vec<f32>,
    tension: f32,
    sample_rate: f32,
    kind: String,
}

#[derive(Serialize)]
struct Output {
    packed: Vec<f32>,
}

pub(super) fn run(args: &[OsString], exponentials: bool) -> Result<(), String> {
    if args.len() != 2 {
        return Err("Usage: roicepeak --process-spectra INPUT_JSON NEW_OUTPUT_JSON".into());
    }
    let input = std::fs::File::open(Path::new(&args[0])).map_err(|error| error.to_string())?;
    if !input
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Spectrum input must be a regular file".into());
    }
    let mut bytes = Vec::new();
    input
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Spectrum input exceeds 16 MiB".into());
    }
    if exponentials {
        let cases: Vec<[u32; 2]> =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if cases.len() > 100_000 {
            return Err("Response exponential case count exceeds 100000".into());
        }
        let output = cases
            .into_iter()
            .map(|[value, scale]| {
                roicepeak::vocoder::response_exp(f32::from_bits(value), f32::from_bits(scale))
                    .map(f32::to_bits)
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;
        return write_output(args, &output);
    }
    let cases: Vec<Case> = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if cases.len() > 256 {
        return Err("Spectrum case count exceeds 256".into());
    }
    let mut output = Vec::with_capacity(cases.len());
    for mut case in cases {
        let kind = match case.kind.as_str() {
            "periodic" => SpectrumKind::Periodic,
            "aperiodic" => SpectrumKind::Aperiodic,
            _ => return Err("Unknown spectrum kind".into()),
        };
        Effects::new(&case.periodic, &case.aperiodic)
            .and_then(|effects| {
                effects.apply(&mut case.packed, kind, case.tension, case.sample_rate)
            })
            .map_err(|error| error.to_string())?;
        output.push(Output {
            packed: case.packed,
        });
    }
    write_output(args, &output)
}

fn write_output<T: Serialize>(args: &[OsString], output: &[T]) -> Result<(), String> {
    let data = serde_json::to_vec(output).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(Path::new(&args[1]))
        .map_err(|error| error.to_string())?;
    file.write_all(&data).map_err(|error| error.to_string())?;
    println!("{{\"cases\":{}}}", output.len());
    Ok(())
}
