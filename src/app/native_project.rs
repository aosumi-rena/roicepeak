//! Bounded VOICEPEAK project I/O with raw fields preserved.
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};
use std::{
    fmt,
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
};

pub const TARGET_VERSION: &str = "1.2.23";
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct NativeProject {
    value: Value,
    original: Option<Vec<u8>>,
}
impl NativeProject {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES {
            return Err("Native project exceeds 16 MiB".into());
        }
        let payload = bytes
            .strip_suffix(&[0])
            .ok_or("Native 1.2.23 project requires its terminal NUL byte")?;
        let Unique(value) = serde_json::from_slice(payload).map_err(|e| e.to_string())?;
        let mut remaining = 500_000;
        bounded(&value, 0, &mut remaining)?;
        let root = object(&value, "document")?;
        if string(root, "version")? != TARGET_VERSION {
            return Err("Native project version is not the observed 1.2.23 schema".into());
        }
        let project = object(required(root, "project")?, "project")?;
        let blocks = array(project, "blocks")?;
        if blocks.len() > 10_000 {
            return Err("Native project exceeds 10,000 blocks".into());
        }
        for (block_index, block) in blocks.iter().enumerate() {
            (|| {
                let block = object(block, "speech block")?;
                let narrator = object(required(block, "narrator")?, "narrator")?;
                string(narrator, "key")?;
                let sentences = array(block, "sentence-list")?;
                for (sentence_index, sentence) in sentences.iter().enumerate() {
                    (|| {
                        let sentence = object(sentence, "sentence")?;
                        string(sentence, "text")?;
                        array(sentence, "tokens")?;
                        Ok::<(), String>(())
                    })()
                    .map_err(|e| format!("sentence-list/{sentence_index}: {e}"))?;
                }
                // Preserve other fields.
                Ok::<(), String>(())
            })()
            .map_err(|e| format!("/project/blocks/{block_index}: {e}"))?;
        }
        Ok(Self {
            value,
            original: Some(bytes.to_vec()),
        })
    }
    pub fn read(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES as u64 {
            return Err("Native project must be a regular file of at most 16 MiB".into());
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        Self::parse(&bytes)
    }
    pub fn root(&self) -> &Value {
        &self.value
    }
    pub fn blocks(&self) -> &[Value] {
        self.value["project"]["blocks"].as_array().unwrap()
    }
    pub fn block(&self, index: usize) -> Result<NativeBlock<'_>, String> {
        let value = self
            .blocks()
            .get(index)
            .ok_or("Native block index outside project")?;
        Ok(NativeBlock {
            value: object(value, "speech block")?,
        })
    }
    /// Replace an existing raw numeric project field. Units and related flags are
    /// the caller's responsibility; this does not generate missing native state.
    pub fn replace_existing_number(&mut self, pointer: &str, number: f64) -> Result<(), String> {
        if !pointer.starts_with("/project/") || pointer.len() > 4096 {
            return Err("Numeric edit must address an existing project field".into());
        }
        let number = Number::from_f64(number).ok_or("Native numeric edit must be finite")?;
        let target = self
            .value
            .pointer_mut(pointer)
            .ok_or("Native numeric field not found")?;
        if !target.is_number() {
            return Err("Native field is not numeric".into());
        }
        *target = Value::Number(number);
        self.original = None;
        Ok(())
    }
    /// Unchanged documents retain their exact original bytes. Edited documents
    /// retain every JSON field, but whitespace and member ordering can change.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let bytes = match &self.original {
            Some(bytes) => bytes.clone(),
            None => {
                let mut bytes = serde_json::to_vec(&self.value).map_err(|e| e.to_string())?;
                bytes.push(0);
                bytes
            }
        };
        if bytes.len() > MAX_BYTES {
            return Err("Native project exceeds 16 MiB".into());
        }
        Ok(bytes)
    }
    pub fn write_new(&self, path: &Path) -> Result<(), String> {
        let bytes = self.to_bytes()?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())
    }
}

pub struct NativeBlock<'a> {
    value: &'a Map<String, Value>,
}
impl<'a> NativeBlock<'a> {
    pub fn raw(&self) -> &'a Map<String, Value> {
        self.value
    }
    pub fn narrator_key(&self) -> Result<&'a str, String> {
        string(
            object(required(self.value, "narrator")?, "narrator")?,
            "key",
        )
    }
    pub fn language(&self) -> Result<&'a str, String> {
        string(
            object(required(self.value, "narrator")?, "narrator")?,
            "language",
        )
    }
    pub fn sentence_count(&self) -> usize {
        self.value["sentence-list"].as_array().unwrap().len()
    }
    pub fn sentence(&self, index: usize) -> Result<NativeSentence<'a>, String> {
        let sentence = array(self.value, "sentence-list")?
            .get(index)
            .ok_or("Native sentence index outside block")?;
        Ok(NativeSentence {
            value: object(sentence, "sentence")?,
        })
    }
    pub fn text(&self) -> Result<String, String> {
        let mut out = String::new();
        for index in 0..self.sentence_count() {
            out.push_str(self.sentence(index)?.text()?);
        }
        Ok(out)
    }
    /// Raw serialized offset.
    pub fn time_offset(&self) -> Result<f64, String> {
        required(self.value, "time-offset")?
            .as_f64()
            .ok_or("Native time-offset is not numeric".into())
    }
}

pub struct NativeSentence<'a> {
    value: &'a Map<String, Value>,
}
pub struct NativeTokenTree {
    pub tokens: Vec<roicepeak::phonemes::Token>,
    pub predict: Vec<Vec<bool>>,
}
impl<'a> NativeSentence<'a> {
    pub fn raw(&self) -> &'a Map<String, Value> {
        self.value
    }
    pub fn text(&self) -> Result<&'a str, String> {
        string(self.value, "text")
    }
    /// Decode the linguistic tree while retaining durations, intonation, edit flags,
    /// and token ranges in raw JSON. Missing engine tokens are not synthesized.
    pub fn linguistic_tokens(&self) -> Result<NativeTokenTree, String> {
        use roicepeak::phonemes::{Phone, Syllable, Token};
        let native_tokens = array(self.value, "tokens")?;
        if native_tokens.len() > 4096 {
            return Err("Native sentence exceeds 4,096 tokens".into());
        }
        let mut tokens = Vec::new();
        let mut predict = Vec::new();
        let mut phone_count = 0usize;
        for token in native_tokens {
            let token = object(token, "token")?;
            let mut syllables = Vec::new();
            let mut mask = Vec::new();
            for syllable in array(token, "syl")? {
                let syllable = object(syllable, "syllable")?;
                let mut phones = Vec::new();
                for phone in array(syllable, "p")? {
                    phone_count += 1;
                    if phone_count > 100_000 {
                        return Err("Native sentence exceeds 100,000 phones".into());
                    }
                    let phone = object(phone, "phone")?;
                    phones.push(Phone {
                        symbol: string(phone, "s")?.to_owned(),
                        nucleus: boolean(phone, "n")?,
                    });
                }
                let code = unsigned(syllable, "a")?;
                roicepeak::phonemes::code_name(code).map_err(|e| e.to_string())?;
                syllables.push(Syllable {
                    reading: string(syllable, "s")?.to_owned(),
                    code,
                    phones,
                });
                mask.push(boolean(syllable, "ig")? && !boolean(syllable, "u")?);
            }
            tokens.push(Token {
                text: string(token, "s")?.to_owned(),
                language: unsigned(token, "lang")?
                    .checked_add(1)
                    .ok_or("Native language ID overflow")?,
                pos: unsigned(token, "pos")?,
                syllables,
            });
            predict.push(mask);
        }
        Ok(NativeTokenTree { tokens, predict })
    }
}

fn required<'a>(value: &'a Map<String, Value>, key: &str) -> Result<&'a Value, String> {
    value
        .get(key)
        .ok_or_else(|| format!("Native project field {key} missing"))
}
fn object<'a>(value: &'a Value, name: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("Native {name} is not an object"))
}
fn string<'a>(value: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    required(value, key)?
        .as_str()
        .ok_or_else(|| format!("Native field {key} is not a string"))
}
fn array<'a>(value: &'a Map<String, Value>, key: &str) -> Result<&'a Vec<Value>, String> {
    required(value, key)?
        .as_array()
        .ok_or_else(|| format!("Native field {key} is not an array"))
}
fn boolean(value: &Map<String, Value>, key: &str) -> Result<bool, String> {
    required(value, key)?
        .as_bool()
        .ok_or_else(|| format!("Native field {key} is not boolean"))
}
fn unsigned(value: &Map<String, Value>, key: &str) -> Result<u32, String> {
    required(value, key)?
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| format!("Native field {key} is not uint32"))
}
fn bounded(value: &Value, depth: usize, remaining: &mut usize) -> Result<(), String> {
    if depth > 64 || *remaining == 0 {
        return Err("Native JSON nesting or value budget exceeded".into());
    }
    *remaining -= 1;
    match value {
        Value::String(s) if s.len() > 1_000_000 => {
            return Err("Native string exceeds byte budget".into());
        }
        Value::Array(values) => {
            if values.len() > 100_000 {
                return Err("Native array exceeds element budget".into());
            }
            for value in values {
                bounded(value, depth + 1, remaining)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if key.len() > 4096 {
                    return Err("Native key exceeds byte budget".into());
                }
                bounded(value, depth + 1, remaining)?;
            }
        }
        _ => {}
    }
    Ok(())
}

// Reject duplicate members instead of silently discarding native fields.
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Json;
        impl<'de> Visitor<'de> for Json {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON with unique object members")
            }
            fn visit_unit<E: de::Error>(self) -> Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Unique, E> {
                Number::from_f64(value)
                    .map(|n| Unique(Value::Number(n)))
                    .ok_or_else(|| E::custom("Nonfinite native number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Unique, E> {
                Ok(Unique(value.into()))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Unique, A::Error> {
                let mut values = Vec::new();
                while let Some(Unique(value)) = access.next_element()? {
                    values.push(value);
                }
                Ok(Unique(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Unique, A::Error> {
                let mut values = Map::new();
                while let Some(key) = access.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom("Duplicate native JSON member"));
                    }
                    let Unique(value) = access.next_value()?;
                    values.insert(key, value);
                }
                Ok(Unique(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Json)
    }
}
