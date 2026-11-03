//! Explicit native-project commands. Dispatch before constructing any GUI state.
use crate::app::native_project::{NativeProject, TARGET_VERSION};
use serde_json::{Value, json};
use std::{ffi::OsString, io::Write, path::Path};

pub enum Dispatch {
    NotHandled,
    Exit(i32),
}

/// Arguments exclude the executable name. Paths retain their OS representation.
pub fn dispatch(args: &[OsString]) -> Dispatch {
    let Some(command) = args.first().and_then(|v| v.to_str()) else {
        return Dispatch::NotHandled;
    };
    if !["--inspect-native-project", "--roundtrip-native-project"].contains(&command) {
        return Dispatch::NotHandled;
    }
    let expected = if command == "--inspect-native-project" {
        2
    } else {
        3
    };
    if args.len() != expected {
        eprintln!(
            "Usage: roicepeak.exe {command} INPUT{}",
            if expected == 3 { " NEW_OUTPUT" } else { "" }
        );
        return Dispatch::Exit(2);
    }
    let result = (|| {
        let input = Path::new(&args[1]);
        let project = NativeProject::read(input)?;
        let report = if expected == 2 {
            inspect(&project, input)
        } else {
            let output = Path::new(&args[2]);
            let bytes = project.to_bytes()?.len();
            project.write_new(output)?;
            json!({
                "operation": "native-project-roundtrip",
                "schema_version": TARGET_VERSION,
                "input": input.to_string_lossy(),
                "output": output.to_string_lossy(),
                "bytes_written": bytes,
                "unchanged_original_bytes": true,
                "native_editor_acceptance_qualified": false,
            })
        };
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut stdout, &report).map_err(|e| e.to_string())?;
        stdout.write_all(b"\n").map_err(|e| e.to_string())
    })();
    match result {
        Ok(()) => Dispatch::Exit(0),
        Err(error) => {
            eprintln!("Native project: {error}");
            Dispatch::Exit(1)
        }
    }
}

fn inspect(project: &NativeProject, input: &Path) -> Value {
    let root = project.root();
    let body = &root["project"];
    let mut issues = Issues::default();
    let mut blocks = Vec::new();
    fields(root, "", &[("voices", Kind::Object)], &mut issues);
    fields(
        body,
        "/project",
        &[
            ("params", Kind::Raw),
            ("emotions", Kind::Raw),
            ("global-settings", Kind::Raw),
            ("export", Kind::Raw),
        ],
        &mut issues,
    );
    for (index, block) in project.blocks().iter().enumerate() {
        let pointer = format!("/project/blocks/{index}");
        fields(
            block,
            &pointer,
            &[
                ("time-offset", Kind::Number),
                ("time-offset-mode", Kind::Integer),
                ("params", Kind::Raw),
                ("emotions", Kind::Raw),
                ("sentence-ranges", Kind::Array),
            ],
            &mut issues,
        );
        fields(
            &block["narrator"],
            &format!("{pointer}/narrator"),
            &[
                ("key", Kind::String),
                ("language", Kind::String),
                ("narrator-version", Kind::Integer),
            ],
            &mut issues,
        );
        let mut sentences = Vec::new();
        // Core block/sentence containers were validated by NativeProject::parse.
        for (sentence_index, sentence) in block["sentence-list"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let sentence_pointer = format!("{pointer}/sentence-list/{sentence_index}");
            fields(
                sentence,
                &sentence_pointer,
                &[("has-eos", Kind::Boolean)],
                &mut issues,
            );
            let tokens = sentence["tokens"].as_array().unwrap();
            let mut syllable_count = 0;
            let mut phone_count = 0;
            for (token_index, token) in tokens.iter().enumerate() {
                let token_pointer = format!("{sentence_pointer}/tokens/{token_index}");
                fields(
                    token,
                    &token_pointer,
                    &[
                        ("s", Kind::String),
                        ("pos", Kind::Unsigned),
                        ("lang", Kind::Unsigned),
                        ("pe", Kind::Boolean),
                        ("syl", Kind::Array),
                        ("r8", Kind::RangePair),
                        ("r32", Kind::RangePair),
                    ],
                    &mut issues,
                );
                for (syllable_index, syllable) in token
                    .get("syl")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    syllable_count += 1;
                    let syllable_pointer = format!("{token_pointer}/syl/{syllable_index}");
                    fields(
                        syllable,
                        &syllable_pointer,
                        &[
                            ("s", Kind::String),
                            ("a", Kind::Unsigned),
                            ("ig", Kind::Boolean),
                            ("u", Kind::Boolean),
                            ("i", Kind::Number),
                            ("p", Kind::Array),
                        ],
                        &mut issues,
                    );
                    for (phone_index, phone) in syllable
                        .get("p")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .enumerate()
                    {
                        phone_count += 1;
                        fields(
                            phone,
                            &format!("{syllable_pointer}/p/{phone_index}"),
                            &[
                                ("s", Kind::String),
                                ("d", Kind::Number),
                                ("dt", Kind::DurationMode),
                                ("n", Kind::Boolean),
                            ],
                            &mut issues,
                        );
                    }
                }
            }
            let decoded = project
                .block(index)
                .and_then(|b| b.sentence(sentence_index))
                .and_then(|s| s.linguistic_tokens());
            let tree = match decoded {
                Ok(tree) => json!({"decoded":true,"tokens":tree.tokens.len(),
                    "prediction_per_syllable":tree.predict,
                    "contains_explicit_bos":tree.tokens.first().is_some_and(|t|t.pos==0x2000),
                    "contains_explicit_eos":tree.tokens.last().is_some_and(|t|t.pos==0x2001)}),
                Err(error) => json!({"decoded":false,"error":error}),
            };
            sentences.push(json!({"index":sentence_index,"text":sentence["text"],
                "token_count":tokens.len(),"syllable_count":syllable_count,
                "phone_count":phone_count,"linguistic_tree":tree}));
        }
        blocks.push(json!({"index":index,"narrator":block["narrator"],
            "time_offset_raw":block.get("time-offset"),
            "time_offset_mode_raw":block.get("time-offset-mode"),"sentences":sentences}));
    }
    json!({
        "operation":"native-project-inspect","input":input.to_string_lossy(),
        "schema_version":TARGET_VERSION,"framing":"UTF-8 JSON followed by terminal NUL",
        "block_count":project.blocks().len(),"blocks":blocks,
        "observed_field_diagnostic_count":issues.total,
        "diagnostics_truncated":issues.total>issues.entries.len(),
        "diagnostics":issues.entries,
        "unit_interpretation":{
            "time-offset":"raw serialized number; application clock/unit unqualified",
            "syllable.i":"raw serialized number; editor transform unqualified",
            "phone.d_dt":"d is authored float64: multiplier when dt=0, absolute seconds when dt=1; dt is an integer mode, distinct from boolean nucleus n",
            "token.r8_r32":"two int32 range endpoints each; coordinate units unqualified"
        },
        "native_editor_acceptance_qualified":false,
        "document":root,
    })
}

#[derive(Default)]
struct Issues {
    total: usize,
    entries: Vec<Value>,
}
impl Issues {
    fn push(&mut self, pointer: String, status: &str, expected: &str, actual: Option<&Value>) {
        self.total += 1;
        if self.entries.len() < 1024 {
            self.entries.push(json!({"pointer":pointer,"status":status,
                "expected":expected,"actual_type":actual.map(type_name)}));
        }
    }
}
#[derive(Clone, Copy)]
enum Kind {
    Raw,
    String,
    Boolean,
    Number,
    Integer,
    Unsigned,
    Array,
    Object,
    RangePair,
    DurationMode,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Raw => "present raw JSON value",
            Self::String => "string",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::Integer => "integer",
            Self::Unsigned => "uint32",
            Self::Array => "array",
            Self::Object => "object",
            Self::RangePair => "two int32 range endpoints",
            Self::DurationMode => "integer duration mode: 0 relative, 1 absolute",
        }
    }
    fn accepts(self, value: &Value) -> bool {
        match self {
            Self::Raw => true,
            Self::String => value.is_string(),
            Self::Boolean => value.is_boolean(),
            Self::Number => value.is_number(),
            Self::Integer => value.as_i64().is_some(),
            Self::Unsigned => value.as_u64().is_some_and(|v| u32::try_from(v).is_ok()),
            Self::Array => value.is_array(),
            Self::Object => value.is_object(),
            Self::RangePair => value.as_array().is_some_and(|v| {
                v.len() == 2
                    && v.iter()
                        .all(|v| v.as_i64().is_some_and(|n| i32::try_from(n).is_ok()))
            }),
            Self::DurationMode => matches!(value.as_i64(), Some(0 | 1)),
        }
    }
}
fn fields(value: &Value, pointer: &str, fields: &[(&str, Kind)], issues: &mut Issues) {
    let Some(object) = value.as_object() else {
        issues.push(pointer.to_owned(), "malformed", "object", Some(value));
        return;
    };
    for &(key, kind) in fields {
        let field_pointer = format!("{pointer}/{key}");
        match object.get(key) {
            None => issues.push(field_pointer, "missing_observed_field", kind.name(), None),
            Some(actual) if !kind.accepts(actual) => {
                issues.push(field_pointer, "malformed", kind.name(), Some(actual))
            }
            _ => {}
        }
    }
}
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
