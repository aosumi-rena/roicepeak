//! Native Japanese user-word classes and bounded in-memory lattice entries.
use super::{bounded_read, dictionary::Record, lookup_step};
use crate::{Result, invalid};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserPartOfSpeech {
    CommonNoun,
    ProperGeneral,
    ProperPerson,
    Surname,
    GivenName,
    Place,
}
impl TryFrom<usize> for UserPartOfSpeech {
    type Error = crate::Error;
    fn try_from(value: usize) -> Result<Self> {
        match value {
            0 => Ok(Self::CommonNoun),
            1 => Ok(Self::ProperGeneral),
            2 => Ok(Self::ProperPerson),
            3 => Ok(Self::Surname),
            4 => Ok(Self::GivenName),
            5 => Ok(Self::Place),
            _ => Err(invalid("unsupported Japanese user-word part of speech")),
        }
    }
}
impl UserPartOfSpeech {
    fn index(self) -> usize {
        self as usize
    }
    fn feature(self) -> &'static str {
        if self == Self::CommonNoun {
            "普通名詞"
        } else {
            "固有名詞"
        }
    }
    fn context(self) -> String {
        let (kind, subkind, origin) = match self {
            Self::CommonNoun => ("一般", "*", "不明"),
            Self::ProperGeneral => ("一般", "*", "固"),
            Self::ProperPerson => ("人名", "一般", "固"),
            Self::Surname => ("人名", "姓", "固"),
            Self::GivenName => ("人名", "名", "固"),
            Self::Place => ("地名", "一般", "固"),
        };
        format!(
            "名詞,{},{kind},{subkind},*,*,*,*,{origin},*,*,*,*,*,*",
            self.feature()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserWord {
    pub spelling: String,
    pub reading: String,
    pub part_of_speech: UserPartOfSpeech,
    pub priority: u8,
    /// Accent nucleus; zero denotes a flat accent.
    pub accent: usize,
    /// Explicit codes are used when their count matches the reading.
    pub overwrite_accents: Vec<i32>,
}
impl UserWord {
    pub fn accent_feature(&self) -> Result<String> {
        if self.overwrite_accents.len() > 4096 {
            return Err(invalid("user accent override count budget"));
        }
        if !self.overwrite_accents.is_empty()
            && self.overwrite_accents.len() == crate::japanese::morae(&self.reading)?.len()
        {
            let marks: String = self
                .overwrite_accents
                .iter()
                .map(|code| if *code == 0x2001 { 'H' } else { 'L' })
                .collect();
            return Ok(format!("{marks}@{}", self.accent));
        }
        Ok(self.accent.to_string())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserDefinitionFile {
    pub path: PathBuf,
    pub bytes: u64,
    pub modified: SystemTime,
}
pub(super) struct Entry {
    pub spelling: String,
    pub feature: String,
    pub record: Record,
}
#[derive(Default)]
struct TrieNode {
    next: BTreeMap<u8, usize>,
    terminal: Vec<usize>,
}

pub struct UserDictionary {
    directory: PathBuf,
    entries: Vec<Entry>,
    trie: Vec<TrieNode>,
    definitions: Vec<UserDefinitionFile>,
}
impl UserDictionary {
    pub(super) fn new(
        directory: &Path,
        dimensions: (usize, usize),
        words: &[UserWord],
    ) -> Result<Self> {
        if words.len() > 10_000 {
            return Err(invalid("user dictionary entry count budget"));
        }
        let mut result = Self {
            directory: directory.into(),
            entries: Vec::new(),
            trie: vec![TrieNode::default()],
            definitions: Vec::new(),
        };
        if words.is_empty() {
            return Ok(result);
        }
        let classes = [
            UserPartOfSpeech::CommonNoun,
            UserPartOfSpeech::ProperGeneral,
            UserPartOfSpeech::ProperPerson,
            UserPartOfSpeech::Surname,
            UserPartOfSpeech::GivenName,
            UserPartOfSpeech::Place,
        ];
        let keys = classes.map(UserPartOfSpeech::context);
        let (left, source) = context_ids(&directory.join("left-id.def"), &keys, dimensions.0)?;
        result.definitions.push(source);
        let (right, source) = context_ids(&directory.join("right-id.def"), &keys, dimensions.1)?;
        result.definitions.push(source);
        let mut total = 0usize;
        for word in words {
            if word.spelling.is_empty()
                || word.spelling.len() > 4096
                || word.reading.is_empty()
                || word.reading.len() > 16_384
                || !(1..=10).contains(&word.priority)
                || word.accent > 4096
                || word.spelling.contains(['\0', '"', '\n', '\r'])
                || word.reading.contains(['\0', '"', '\n', '\r'])
            {
                return Err(invalid(
                    "invalid Japanese user-word text, priority or accent",
                ));
            }
            total = total
                .checked_add(word.spelling.len() + word.reading.len())
                .ok_or_else(|| invalid("user dictionary byte size overflow"))?;
            if total > 1_048_576 {
                return Err(invalid("user dictionary text byte budget"));
            }
            let class = word.part_of_speech.index();
            let feature = format!(
                "名詞,{},{},{},*,*,1",
                word.part_of_speech.feature(),
                csv(&word.reading),
                word.accent_feature()?
            );
            let index = result.entries.len();
            result.entries.push(Entry {
                spelling: word.spelling.clone(),
                feature,
                record: Record {
                    lc: left[class],
                    rc: right[class],
                    cost: -1000 * i16::from(word.priority),
                    feature: 0,
                },
            });
            let mut at = 0;
            for byte in word.spelling.bytes() {
                at = if let Some(next) = result.trie[at].next.get(&byte) {
                    *next
                } else {
                    let next = result.trie.len();
                    result.trie.push(TrieNode::default());
                    result.trie[at].next.insert(byte, next);
                    next
                };
            }
            result.trie[at].terminal.push(index);
        }
        Ok(result)
    }
    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn definition_files(&self) -> &[UserDefinitionFile] {
        &self.definitions
    }
    pub(super) fn entry(&self, index: usize) -> &Entry {
        &self.entries[index]
    }
    pub(super) fn prefixes(&self, text: &str, steps: &mut usize) -> Result<Vec<usize>> {
        let mut at = 0;
        let mut found = Vec::new();
        for byte in text.bytes() {
            lookup_step(steps)?;
            let Some(next) = self.trie[at].next.get(&byte) else {
                break;
            };
            at = *next;
            found.extend_from_slice(&self.trie[at].terminal);
            if found.len() > 8192 {
                return Err(invalid("user dictionary candidate budget"));
            }
        }
        Ok(found)
    }
}

fn csv(value: &str) -> String {
    if value.contains(',') {
        format!("\"{value}\"")
    } else {
        value.into()
    }
}

fn context_ids(
    path: &Path,
    keys: &[String; 6],
    count: usize,
) -> Result<([u16; 6], UserDefinitionFile)> {
    let metadata = std::fs::metadata(path)?;
    let source = UserDefinitionFile {
        path: std::fs::canonicalize(path)?,
        bytes: metadata.len(),
        modified: metadata.modified()?,
    };
    let bytes = bounded_read(path, 8 * 1024 * 1024)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| invalid("context ID definitions must be UTF-8"))?;
    let mut found = [None; 6];
    let mut lines = 0usize;
    for line in text.lines() {
        lines += 1;
        if lines > 65_536 || line.len() > 65_536 {
            return Err(invalid("context ID definition budget"));
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (id, feature) = line
            .split_once(char::is_whitespace)
            .ok_or_else(|| invalid("context ID row requires ID and feature"))?;
        let id = id
            .parse::<u16>()
            .map_err(|_| invalid("invalid context ID"))?;
        if usize::from(id) >= count {
            return Err(invalid("context ID exceeds dictionary dimensions"));
        }
        if let Some(index) = keys.iter().position(|key| key == feature.trim_start()) {
            if found[index].replace(id).is_some() {
                return Err(invalid("duplicate user-word context definition"));
            }
        }
    }
    let mut result = [0u16; 6];
    for (index, value) in found.into_iter().enumerate() {
        result[index] = value.ok_or_else(|| {
            invalid(format!(
                "missing user-word context class {} in {}",
                index,
                path.display()
            ))
        })?;
    }
    let after = std::fs::metadata(path)?;
    if after.len() != source.bytes || after.modified()? != source.modified {
        return Err(invalid("context ID definitions changed while loading"));
    }
    Ok((result, source))
}
