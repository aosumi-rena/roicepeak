use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
const MAX_INPUTS: usize = 4096;
const MAX_MORAE: usize = 100_000;
const MAX_PHONES: usize = 300_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    inputs: Vec<String>,
}

pub(super) fn run(args: &[OsString]) -> Result<(), String> {
    if !args.is_empty() {
        return Err("--inspect-reading accepts only a JSON batch on stdin".into());
    }
    let mut input = Vec::new();
    io::stdin()
        .lock()
        .take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut input)
        .map_err(|e| e.to_string())?;
    if input.len() > MAX_INPUT_BYTES {
        return Err("Reading batch exceeds 8 MiB".into());
    }
    let batch: Batch = serde_json::from_slice(&input).map_err(|e| e.to_string())?;
    if batch.inputs.len() > MAX_INPUTS {
        return Err("Reading batch exceeds 4096 inputs".into());
    }
    let mut mora_count = 0usize;
    let mut phone_count = 0usize;
    let mut results = Vec::with_capacity(batch.inputs.len());
    for text in batch.inputs {
        let result = match roicepeak::japanese::edit_reading(&text) {
            Ok(syllables) => {
                mora_count = mora_count
                    .checked_add(syllables.len())
                    .filter(|n| *n <= MAX_MORAE)
                    .ok_or("Reading batch mora budget")?;
                let normalized = syllables
                    .iter()
                    .map(|m| m.reading.as_str())
                    .collect::<String>();
                let mut rendered = Vec::with_capacity(syllables.len());
                for mora in syllables {
                    phone_count = phone_count
                        .checked_add(mora.phones.len())
                        .filter(|n| *n <= MAX_PHONES)
                        .ok_or("Reading batch phone budget")?;
                    let phones = mora
                        .phones
                        .into_iter()
                        .map(|phone| json!({"symbol": phone.symbol, "nucleus": phone.nucleus}))
                        .collect::<Vec<_>>();
                    rendered.push(
                        json!({"reading": mora.reading, "code": mora.code, "phones": phones}),
                    );
                }
                json!({"input": text, "valid": true, "normalized": normalized, "syllables": rendered})
            }
            Err(error) => json!({"input": text, "valid": false, "error": error.to_string()}),
        };
        results.push(result);
    }
    let mut output = serde_json::to_vec(&json!({"results": results})).map_err(|e| e.to_string())?;
    if output.len() > MAX_OUTPUT_BYTES {
        return Err("Reading batch output exceeds 64 MiB".into());
    }
    output.push(b'\n');
    io::stdout()
        .lock()
        .write_all(&output)
        .map_err(|e| e.to_string())
}
