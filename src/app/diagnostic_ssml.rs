use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    inputs: Vec<String>,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-ssml-reading accepts a JSON batch on stdin".into());
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("SSML reading batch exceeds 8 MiB".into());
    }
    let batch: Batch = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if batch.inputs.len() > 4096 {
        return Err("SSML reading batch exceeds 4096 inputs".into());
    }
    let (mut morae, mut phones, mut groups) = (0usize, 0usize, 0usize);
    let mut results = Vec::with_capacity(batch.inputs.len());
    for text in batch.inputs {
        let value = match roicepeak::japanese::ssml_reading(&text) {
            Ok(reading) => {
                morae = morae
                    .checked_add(reading.morae.len())
                    .filter(|n| *n <= 100_000)
                    .ok_or("SSML batch mora budget")?;
                groups = groups
                    .checked_add(reading.groups.len())
                    .filter(|n| *n <= 100_000)
                    .ok_or("SSML batch group budget")?;
                let mut rendered = Vec::with_capacity(reading.morae.len());
                for mora in reading.morae {
                    phones = phones
                        .checked_add(mora.syllable.phones.len())
                        .filter(|n| *n <= 300_000)
                        .ok_or("SSML batch phone budget")?;
                    let parsed = mora
                        .syllable
                        .phones
                        .into_iter()
                        .map(|phone| json!({"symbol":phone.symbol,"nucleus":phone.nucleus}))
                        .collect::<Vec<_>>();
                    rendered.push(json!({"reading":mora.syllable.reading,"code":mora.syllable.code,
                        "prediction_eligible":mora.prediction_eligible,"accent_edited":mora.accent_edited,"phones":parsed}));
                }
                let groups = reading
                    .groups
                    .into_iter()
                    .map(|range| json!({"start":range.start,"end":range.end}))
                    .collect::<Vec<_>>();
                json!({"input":text,"valid":true,"morae":rendered,"groups":groups})
            }
            Err(error) => json!({"input":text,"valid":false,"error":error.to_string()}),
        };
        results.push(value);
    }
    let mut output =
        serde_json::to_vec(&json!({"results":results})).map_err(|error| error.to_string())?;
    if output.len() > 64 * 1024 * 1024 {
        return Err("SSML reading output exceeds 64 MiB".into());
    }
    output.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&output)
        .map_err(|error| error.to_string())
}
