//! Explicit project-owned pronunciation entries, applied before morphology.
use super::{Analysis, Word, accent, finish, morae, word};
use crate::{Result, frontend::Tagger, invalid};
use std::collections::BTreeMap;

/// A validated pronunciation entry. This is not an installed/global user
/// dictionary file and does not discover application preferences.
pub struct Pronunciation {
    spelling: String,
    reading: String,
    accent: usize,
}

impl Pronunciation {
    pub fn new(spelling: &str, reading: &str, accent: usize) -> Result<Self> {
        let spelling = spelling.trim();
        let reading = reading.trim();
        if spelling.is_empty() || spelling.len() > 4096 || spelling.contains('\0') {
            return Err(invalid(
                "pronunciation entry has empty, overlong or NUL spelling",
            ));
        }
        if reading.len() > 16_384 {
            return Err(invalid("pronunciation entry reading byte budget"));
        }
        let count = morae(reading)?.len();
        if count == 0 || accent > count {
            return Err(invalid(
                "pronunciation entry accent must be within zero and its mora count",
            ));
        }
        Ok(Self {
            spelling: spelling.into(),
            reading: reading.into(),
            accent,
        })
    }

    pub fn spelling(&self) -> &str {
        &self.spelling
    }
    pub fn reading(&self) -> &str {
        &self.reading
    }
    pub fn accent(&self) -> usize {
        self.accent
    }

    fn word(&self) -> Result<Word> {
        let mut result = Word::new(&self.spelling, "名詞", "固有名詞", &self.reading)?;
        result.accent = self.accent as i32;
        result.explicit = Some(
            accent::pattern(result.morae.len(), result.accent)
                .into_iter()
                .map(|high| if high { 'H' } else { 'L' })
                .collect(),
        );
        result.fixed = true;
        Ok(result)
    }
}

#[derive(Default)]
struct Node {
    next: BTreeMap<u8, usize>,
    terminal: Option<usize>,
}

fn trie(entries: &[Pronunciation]) -> Result<Vec<Node>> {
    if entries.len() > 4096 || entries.iter().map(|e| e.spelling.len()).sum::<usize>() > 65_536 {
        return Err(invalid(
            "explicit pronunciation dictionary byte/entry budget",
        ));
    }
    let mut nodes = vec![Node::default()];
    for (index, entry) in entries.iter().enumerate() {
        let mut at = 0;
        for byte in entry.spelling.bytes() {
            at = if let Some(next) = nodes[at].next.get(&byte) {
                *next
            } else {
                let next = nodes.len();
                nodes.push(Node::default());
                nodes[at].next.insert(byte, next);
                next
            };
        }
        if nodes[at].terminal.replace(index).is_some() {
            return Err(invalid("duplicate explicit pronunciation spelling"));
        }
    }
    Ok(nodes)
}

fn append(tagger: &Tagger, text: &str, base: usize, words: &mut Vec<Word>) -> Result<()> {
    if !text.is_empty() {
        for mut morph in tagger.parse(text)? {
            morph.start += base;
            morph.end += base;
            words.push(word(&morph)?);
        }
    }
    if words.len() > 4094 {
        return Err(invalid("Japanese token budget"));
    }
    Ok(())
}

/// Match explicit entries leftmost, longest at that position, before tagger
/// analysis. `normalized` must already have passed `normalize` exactly once;
/// entry spellings match that normalized text literally. Context around an
/// explicit entry is tagged independently, as in the observed override path.
pub fn analyze_text(
    tagger: &Tagger,
    normalized: &str,
    entries: &[Pronunciation],
) -> Result<Analysis> {
    if normalized.len() > 65_536 || normalized.chars().count() > 8192 || normalized.contains('\0') {
        return Err(invalid("Japanese explicit-override input budget or NUL"));
    }
    let nodes = trie(entries)?;
    let bytes = normalized.as_bytes();
    let mut words = Vec::new();
    let mut start = 0;
    let mut at = 0;
    let mut transitions = 0usize;
    while at < bytes.len() {
        let mut node = 0;
        let mut longest = None;
        for byte in &bytes[at..] {
            transitions += 1;
            if transitions > 16_000_000 {
                return Err(invalid("pronunciation matching transition budget"));
            }
            let Some(next) = nodes[node].next.get(byte) else {
                break;
            };
            node = *next;
            if let Some(index) = nodes[node].terminal {
                longest = Some(index);
            }
        }
        if let Some(index) = longest {
            append(tagger, &normalized[start..at], start, &mut words)?;
            let mut matched = entries[index].word()?;
            matched.input_range = Some(at..at + entries[index].spelling.len());
            words.push(matched);
            if words.len() > 4094 {
                return Err(invalid("Japanese token budget"));
            }
            at += entries[index].spelling.len();
            start = at;
        } else {
            at += normalized[at..].chars().next().unwrap().len_utf8();
        }
    }
    append(tagger, &normalized[start..], start, &mut words)?;
    let mut analysis = finish(words)?;
    analysis.input_text = Some(normalized.to_owned());
    Ok(analysis)
}
