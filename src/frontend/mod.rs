//! Bounded morphological analysis of an explicitly supplied UTF-8 dictionary.
//!
//! No dictionary discovery, text normalization or pronunciation guessing occurs here.
mod chars;
mod dictionary;
mod matrix;
mod normalization;
mod user;
pub use user::{UserDefinitionFile, UserDictionary, UserPartOfSpeech, UserWord};

use crate::{Result, invalid};
use chars::Characters;
use dictionary::{Dictionary, Record};
use matrix::Matrix;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

const MAX_INPUT_BYTES: usize = 65_536;
const MAX_INPUT_CHARS: usize = 8_192;
const MAX_NODES: usize = 262_144;
const MAX_CANDIDATES: usize = 8_192;
const MAX_TRANSITIONS: usize = 16_000_000;
const MAX_CONNECTIONS: usize = 250_000;

/// One selected dictionary record; offsets are UTF-8 byte offsets in the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Morph {
    pub surface: String,
    pub feature: String,
    pub lc: u16,
    pub rc: u16,
    pub cost: i16,
    pub start: usize,
    pub end: usize,
    pub unknown: bool,
}

pub struct Tagger {
    directory: PathBuf,
    lexicons: Vec<Dictionary>,
    unknown: Dictionary,
    chars: Characters,
    categories: Vec<Vec<Record>>,
    matrix: Matrix,
}

#[derive(Clone, Copy)]
struct Candidate {
    dictionary: usize,
    record: Record,
    start: usize,
    end: usize,
    unknown: bool,
    user: Option<usize>,
}

struct Node {
    candidate: Option<Candidate>,
    cost: i64,
    rc: u16,
    previous: Option<usize>,
}

impl Tagger {
    /// Open only the specified directory and optional explicit user dictionaries.
    pub fn open(dic: &Path, user: &[PathBuf]) -> Result<Self> {
        if user.len() > 8 {
            return Err(invalid("at most eight explicit user dictionaries"));
        }
        let system = Dictionary::open(&dic.join("sys.dic"), 0)?;
        let mut lexicons = vec![system];
        for path in user {
            let dictionary = Dictionary::open(path, 1)?;
            if dictionary.dimensions() != lexicons[0].dimensions() {
                return Err(invalid("user dictionary connection dimensions differ"));
            }
            lexicons.push(dictionary);
        }
        let unknown = Dictionary::open(&dic.join("unk.dic"), 2)?;
        if unknown.dimensions() != lexicons[0].dimensions() {
            return Err(invalid("unknown dictionary connection dimensions differ"));
        }
        let chars = Characters::open(&dic.join("char.bin"))?;
        let mut categories = Vec::new();
        for category in chars.names() {
            let value = unknown
                .exact(category.as_bytes())?
                .ok_or_else(|| invalid("unknown dictionary lacks character category"))?;
            categories.push(unknown.records(value)?);
        }
        let (left_ids, right_ids) = lexicons[0].dimensions();
        let matrix = Matrix::open(dic, right_ids, left_ids)?;
        Ok(Self {
            directory: std::fs::canonicalize(dic)?,
            lexicons,
            unknown,
            chars,
            categories,
            matrix,
        })
    }

    fn candidates(
        &self,
        text: &str,
        start: usize,
        steps: &mut usize,
        user: Option<&UserDictionary>,
    ) -> Result<Vec<Candidate>> {
        let mut out = Vec::new();
        for (index, dictionary) in self.lexicons.iter().enumerate() {
            for (value, length) in dictionary.prefixes(text[start..].as_bytes(), steps)? {
                let end = start + length;
                if length == 0 || !text.is_char_boundary(end) {
                    continue;
                }
                for record in dictionary.records(value)? {
                    out.push(Candidate {
                        dictionary: index,
                        record,
                        start,
                        end,
                        unknown: false,
                        user: None,
                    });
                    if out.len() > MAX_CANDIDATES {
                        return Err(invalid("lexical candidate budget exceeded"));
                    }
                }
            }
        }
        if let Some(user) = user {
            for index in user.prefixes(&text[start..], steps)? {
                let entry = user.entry(index);
                out.push(Candidate {
                    dictionary: 0,
                    record: entry.record,
                    start,
                    end: start + entry.spelling.len(),
                    unknown: false,
                    user: Some(index),
                });
                if out.len() > MAX_CANDIDATES {
                    return Err(invalid("lexical candidate budget exceeded"));
                }
            }
        }
        let first = text[start..]
            .chars()
            .next()
            .ok_or_else(|| invalid("empty candidate suffix"))?;
        let info = self.chars.info(first);
        if info.invoke || out.is_empty() {
            let mut lengths = Vec::new();
            let mut characters = 0usize;
            let mut group_end = start;
            for (offset, ch) in text[start..].char_indices() {
                lookup_step(steps)?;
                if self.chars.info(ch).mask & info.mask == 0 {
                    break;
                }
                characters += 1;
                group_end = start + offset + ch.len_utf8();
                if characters <= info.length {
                    lengths.push(group_end);
                }
                if !info.group && characters >= info.length {
                    break;
                }
            }
            if info.group && group_end > start {
                lengths.push(group_end);
            }
            lengths.sort_unstable();
            lengths.dedup();
            for end in lengths {
                for &record in &self.categories[info.category] {
                    out.push(Candidate {
                        dictionary: 0,
                        record,
                        start,
                        end,
                        unknown: true,
                        user: None,
                    });
                    if out.len() > MAX_CANDIDATES {
                        return Err(invalid("unknown candidate budget exceeded"));
                    }
                }
            }
        }
        Ok(out)
    }

    /// Find the minimum-cost path with stable dictionary/token-order ties.
    /// BOS/EOS participate in costs but are not returned as surface records.
    pub fn parse(&self, text: &str) -> Result<Vec<Morph>> {
        self.parse_impl(text, None)
    }

    /// Resolve the six native user-word context classes against this dictionary.
    /// Definitions are read only for a nonempty explicit entry list.
    pub fn user_dictionary(&self, words: &[UserWord]) -> Result<UserDictionary> {
        UserDictionary::new(&self.directory, self.lexicons[0].dimensions(), words)
    }

    /// User words participate in the same minimum-cost lattice as system words.
    pub fn parse_with_user(&self, text: &str, user: &UserDictionary) -> Result<Vec<Morph>> {
        if user.directory() != self.directory.as_path() {
            return Err(invalid("user entries belong to a different dictionary"));
        }
        self.parse_impl(text, Some(user))
    }

    fn parse_impl(&self, text: &str, user: Option<&UserDictionary>) -> Result<Vec<Morph>> {
        if text.len() > MAX_INPUT_BYTES || text.chars().count() > MAX_INPUT_CHARS {
            return Err(invalid("lexical input budget exceeded"));
        }
        if text.contains('\0') {
            return Err(invalid("NUL in lexical input"));
        }
        let mut ends = vec![Vec::<usize>::new(); text.len() + 1];
        let mut nodes = vec![Node {
            candidate: None,
            cost: 0,
            rc: 0,
            previous: None,
        }];
        ends[0].push(0);
        let mut connections = HashMap::new();
        let mut transitions = 0usize;
        let mut lookup_steps = 0usize;
        for (position, _) in text.char_indices() {
            if ends[position].is_empty() {
                continue;
            }
            let mut start = position;
            for ch in text[position..].chars() {
                if !self.chars.is_space(ch) {
                    break;
                }
                start += ch.len_utf8();
            }
            if start == text.len() {
                let previous = ends[position].clone();
                ends[start].extend(previous);
                continue;
            }
            for candidate in self.candidates(text, start, &mut lookup_steps, user)? {
                let mut best: Option<(i64, usize)> = None;
                for &previous in &ends[position] {
                    transitions += 1;
                    if transitions > MAX_TRANSITIONS {
                        return Err(invalid("Viterbi transition budget exceeded"));
                    }
                    let connection = connection(
                        &self.matrix,
                        &mut connections,
                        nodes[previous].rc,
                        candidate.record.lc,
                    )?;
                    let cost = nodes[previous]
                        .cost
                        .checked_add(connection)
                        .and_then(|c| c.checked_add(i64::from(candidate.record.cost)))
                        .ok_or_else(|| invalid("Viterbi cost overflow"))?;
                    if best.is_none_or(|(old, _)| cost < old) {
                        best = Some((cost, previous));
                    }
                }
                if let Some((cost, previous)) = best {
                    if nodes.len() >= MAX_NODES {
                        return Err(invalid("Viterbi node budget exceeded"));
                    }
                    ends[candidate.end].push(nodes.len());
                    nodes.push(Node {
                        candidate: Some(candidate),
                        cost,
                        rc: candidate.record.rc,
                        previous: Some(previous),
                    });
                }
            }
        }
        let mut best: Option<(i64, usize)> = None;
        for &index in &ends[text.len()] {
            transitions += 1;
            if transitions > MAX_TRANSITIONS {
                return Err(invalid("Viterbi transition budget exceeded"));
            }
            let cost = nodes[index]
                .cost
                .checked_add(connection(
                    &self.matrix,
                    &mut connections,
                    nodes[index].rc,
                    0,
                )?)
                .ok_or_else(|| invalid("EOS cost overflow"))?;
            if best.is_none_or(|(old, _)| cost < old) {
                best = Some((cost, index));
            }
        }
        let mut index = best.ok_or_else(|| invalid("no complete lexical path"))?.1;
        let mut result = Vec::new();
        while let Some(candidate) = nodes[index].candidate {
            let feature = if let Some(index) = candidate.user {
                user.ok_or_else(|| invalid("missing user dictionary for selected path"))?
                    .entry(index)
                    .feature
                    .as_str()
            } else {
                let dictionary = if candidate.unknown {
                    &self.unknown
                } else {
                    &self.lexicons[candidate.dictionary]
                };
                dictionary.feature(candidate.record.feature)?
            };
            result.push(Morph {
                surface: text[candidate.start..candidate.end].to_owned(),
                feature: feature.to_owned(),
                lc: candidate.record.lc,
                rc: candidate.record.rc,
                cost: candidate.record.cost,
                start: candidate.start,
                end: candidate.end,
                unknown: candidate.unknown,
            });
            index = nodes[index]
                .previous
                .ok_or_else(|| invalid("broken lexical predecessor"))?;
        }
        result.reverse();
        Ok(result)
    }
}

fn connection(
    matrix: &Matrix,
    cache: &mut HashMap<(u16, u16), i64>,
    rc: u16,
    lc: u16,
) -> Result<i64> {
    if let Some(&cost) = cache.get(&(rc, lc)) {
        return Ok(cost);
    }
    if cache.len() >= MAX_CONNECTIONS {
        return Err(invalid("connection cost budget exceeded"));
    }
    let cost = matrix.cost(rc, lc)?;
    cache.insert((rc, lc), cost);
    Ok(cost)
}

pub(super) fn bounded_read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > limit as u64 {
        return Err(invalid("dictionary input must be a bounded regular file"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid("dictionary input grew beyond budget"));
    }
    Ok(bytes)
}

pub(super) fn lookup_step(steps: &mut usize) -> Result<()> {
    *steps += 1;
    if *steps > 16_000_000 {
        return Err(invalid("dictionary lookup operation budget exceeded"));
    }
    Ok(())
}

pub(super) fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
