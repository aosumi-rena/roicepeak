use crate::app::{
    document::{Project, VoiceRef, WordState, ssml::Pending},
    sentence_edit::WordPositions,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    io::{Read, Write},
};

const INPUT_LIMIT: usize = 4 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 64 * 1024 * 1024;
const CASE_LIMIT: usize = 64;
const TEXT_LIMIT: usize = 1_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch<T> {
    cases: Vec<T>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportCase {
    source: String,
    narrator: VoiceRef,
    export_settings: crate::app::export::Settings,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnnotationCase {
    pending: Pending,
    positions: WordPositions,
    words: Vec<WordState>,
}

#[derive(Serialize)]
struct CaseResult<T> {
    value: Option<T>,
    error: Option<String>,
}

impl<T> From<Result<T, String>> for CaseResult<T> {
    fn from(value: Result<T, String>) -> Self {
        match value {
            Ok(value) => Self {
                value: Some(value),
                error: None,
            },
            Err(error) => Self {
                value: None,
                error: Some(error),
            },
        }
    }
}

#[derive(Default)]
struct Budget {
    words: usize,
    morae: usize,
    phones: usize,
    annotations: usize,
    text: usize,
}

impl Budget {
    fn add(count: &mut usize, value: usize, limit: usize) -> Result<(), String> {
        *count = count
            .checked_add(value)
            .ok_or("SSML inspection count overflow")?;
        if *count > limit {
            return Err("SSML inspection aggregate limit exceeded".into());
        }
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<(), String> {
        if text.len() > TEXT_LIMIT || text.contains('\0') {
            return Err("SSML inspection string limit exceeded".into());
        }
        Self::add(&mut self.text, text.len(), 16 * 1024 * 1024)
    }
    fn morae(&mut self, morae: &[crate::app::document::state::MoraState]) -> Result<(), String> {
        Self::add(&mut self.morae, morae.len(), 100_000)?;
        for mora in morae {
            self.text(&mora.reading)?;
            if !mora.intonation_delta.is_finite() {
                return Err("SSML inspection contains nonfinite intonation".into());
            }
            Self::add(&mut self.phones, mora.phones.len(), 400_000)?;
            for phone in &mora.phones {
                self.text(&phone.symbol)?;
                if !phone.duration.value.is_finite() {
                    return Err("SSML inspection contains nonfinite duration".into());
                }
            }
        }
        Ok(())
    }
    fn words(&mut self, words: &[WordState]) -> Result<(), String> {
        Self::add(&mut self.words, words.len(), 65_536)?;
        for word in words {
            self.text(&word.surface)?;
            self.morae(&word.morae)?;
        }
        Ok(())
    }
    fn pending(&mut self, pending: &Pending) -> Result<(), String> {
        self.text(&pending.source)?;
        Self::add(&mut self.annotations, pending.annotations.len(), 100_000)?;
        for annotation in &pending.annotations {
            self.text(&annotation.surface)?;
            self.text(&annotation.ph)?;
            self.morae(&annotation.morae)?;
        }
        Ok(())
    }
    fn project(&mut self, project: &Project) -> Result<(), String> {
        for block in &project.blocks {
            self.text(&block.text)?;
            for sentence in &block.sentences {
                self.text(&sentence.text)?;
                self.words(&sentence.words)?;
                if let Some(pending) = &sentence.ssml {
                    self.pending(pending)?;
                }
            }
        }
        Ok(())
    }
}

struct Output(Vec<u8>);
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > OUTPUT_LIMIT)
        {
            return Err(std::io::Error::other(
                "SSML inspection output exceeds 64 MiB",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn read<T: serde::de::DeserializeOwned>(args: &[OsString]) -> Result<Batch<T>, String> {
    if !args.is_empty() {
        return Err("SSML inspection accepts JSON on stdin and no arguments".into());
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(INPUT_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > INPUT_LIMIT {
        return Err("SSML inspection input exceeds 4 MiB".into());
    }
    let batch: Batch<T> = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if batch.cases.is_empty() || batch.cases.len() > CASE_LIMIT {
        return Err("SSML inspection needs 1 through 64 cases".into());
    }
    Ok(batch)
}

fn write<T: Serialize>(
    cases: impl IntoIterator<Item = Result<T, String>>,
    scope: &str,
) -> Result<(), String> {
    let mut output = Output(Vec::new());
    output
        .write_all(b"{\"cases\":[")
        .map_err(|error| error.to_string())?;
    for (index, case) in cases.into_iter().enumerate() {
        if index != 0 {
            output.write_all(b",").map_err(|error| error.to_string())?;
        }
        serde_json::to_writer(&mut output, &CaseResult::from(case))
            .map_err(|error| error.to_string())?;
    }
    output
        .write_all(b"],\"scope\":")
        .map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut output, scope).map_err(|error| error.to_string())?;
    output
        .write_all(b"}\n")
        .map_err(|error| error.to_string())?;
    std::io::stdout()
        .lock()
        .write_all(&output.0)
        .map_err(|error| error.to_string())
}

pub(super) fn parse(args: &[OsString]) -> Result<(), String> {
    let batch = read::<ImportCase>(args)?;
    let mut budget = Budget::default();
    write(
        batch.cases.into_iter().map(|case| {
            budget.text(&case.source)?;
            let project = crate::app::imports::stage_ssml(
                &case.source,
                &case.narrator,
                case.export_settings,
            )?;
            budget.project(&project)?;
            Ok(project)
        }),
        "production SSML parse and replacement; no catalog or inference",
    )
}

pub(super) fn annotations(args: &[OsString]) -> Result<(), String> {
    let batch = read::<AnnotationCase>(args)?;
    let mut budget = Budget::default();
    write(
        batch.cases.into_iter().map(|mut case| {
            budget.pending(&case.pending)?;
            budget.text(&case.positions.source)?;
            budget.words(&case.words)?;
            if case.words.len() > 4096 || case.positions.ranges.len() != case.words.len() {
                return Err("SSML inspection requires aligned word positions".into());
            }
            case.positions.validate(&case.words)?;
            crate::app::document::ssml::apply(&case.pending, &case.positions, &mut case.words)?;
            budget.words(&case.words)?;
            Ok(case.words)
        }),
        "raw word state through production annotation consumer; no canonical project validity claim",
    )
}
