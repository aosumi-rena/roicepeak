//! Dictionary-only preparation of the canonical native Save snapshot.
use crate::app::{
    dictionary,
    document::{Project, SentenceState, WordState, alignment, ssml, state},
    sentence_edit::WordPositions,
};
use roicepeak::{
    frontend::{Tagger, UserDictionary, UserPartOfSpeech, UserWord},
    japanese::{self, Pronunciation},
};
use std::path::Path;

fn current(sentence: &SentenceState) -> bool {
    sentence.word_positions.source == sentence.text
        && sentence.word_positions.ranges.len() == sentence.words.len()
        && sentence.word_positions.validate(&sentence.words).is_ok()
}

pub(crate) fn required(project: &Project) -> Result<bool, String> {
    for block in &project.blocks {
        if block.sentences.is_empty() {
            if !block.text.is_empty() {
                return Ok(true);
            }
            continue;
        }
        let mut remainder = block.text.as_str();
        for sentence in &block.sentences {
            let Some(rest) = remainder.strip_prefix(sentence.text.as_str()) else {
                return Ok(true);
            };
            remainder = rest;
            if sentence.ssml.is_some() || !current(sentence) {
                return Ok(true);
            }
        }
        if !remainder.is_empty() {
            return Ok(true);
        }
    }
    project.validate()?;
    Ok(false)
}

fn dictionary_words(snapshot: &dictionary::Snapshot) -> Result<Vec<UserWord>, String> {
    snapshot.validate()?;
    snapshot
        .entries()
        .iter()
        .filter(|entry| !entry.native.opaque)
        .map(|entry| {
            let reading = Pronunciation::new(&entry.word, &entry.reading, entry.accent)
                .map_err(|e| e.to_string())?;
            Ok(UserWord {
                spelling: reading.spelling().into(),
                reading: reading.reading().into(),
                accent: reading.accent(),
                part_of_speech: UserPartOfSpeech::try_from(entry.part_of_speech)
                    .map_err(|e| e.to_string())?,
                priority: entry.priority,
                overwrite_accents: entry.native.overwrite_accents.clone(),
            })
        })
        .collect()
}

fn boundary(word: &WordState) -> bool {
    matches!(word.pos, 0x2000 | 0x2001)
}

fn retainable(old: &WordState, new: &WordState) -> bool {
    old.surface == new.surface
        && old.language == new.language
        && old.pos == new.pos
        && (old.pronunciation_edited
            || (old.morae.len() == new.morae.len()
                && old.morae.iter().zip(&new.morae).all(|(a, b)| {
                    a.reading == b.reading
                        && a.phones.len() == b.phones.len()
                        && a.phones
                            .iter()
                            .zip(&b.phones)
                            .all(|(a, b)| a.symbol == b.symbol && a.nucleus == b.nucleus)
                })))
}

fn preserve(saved: &[WordState], fresh: &mut [WordState]) -> Result<(), String> {
    let ordinary = |words: &[WordState]| {
        words
            .iter()
            .enumerate()
            .filter_map(|(index, word)| (!boundary(word)).then_some(index))
            .collect::<Vec<_>>()
    };
    let old = ordinary(saved);
    let new = ordinary(fresh);
    let changes = alignment::align(
        &old.iter()
            .map(|i| saved[*i].surface.as_str())
            .collect::<Vec<_>>(),
        &new.iter()
            .map(|i| fresh[*i].surface.as_str())
            .collect::<Vec<_>>(),
    )?;
    for change in changes {
        if change.kind != 0 {
            continue;
        }
        if let (Some(a), Some(b)) = (change.old, change.new) {
            if retainable(&saved[old[a]], &fresh[new[b]]) {
                fresh[new[b]] = saved[old[a]].clone();
            }
        }
    }
    for word in fresh.iter_mut().filter(|word| boundary(word)) {
        if let Some(old) = saved.iter().find(|old| retainable(old, word)) {
            *word = old.clone();
        }
    }
    Ok(())
}

pub(crate) fn prepare(
    project: &Project,
    dictionary_path: &Path,
    snapshot: &dictionary::Snapshot,
) -> Result<Project, String> {
    if !required(project)? {
        return Ok(project.clone());
    }
    let mut prepared = project.clone();
    let mut lexical: Option<(Tagger, UserDictionary)> = None;
    for block in &mut prepared.blocks {
        block.sync_sentences()?;
        for sentence in &mut block.sentences {
            if current(sentence) && sentence.ssml.is_none() {
                continue;
            }
            let old_words = sentence.words.clone();
            let old_positions = sentence.word_positions.clone();
            if !current(sentence) {
                if lexical.is_none() {
                    let tagger = Tagger::open(dictionary_path, &[]).map_err(|e| e.to_string())?;
                    let user = tagger
                        .user_dictionary(&dictionary_words(snapshot)?)
                        .map_err(|e| e.to_string())?;
                    lexical = Some((tagger, user));
                }
                let (tagger, user) = lexical.as_ref().ok_or("Save dictionary is unavailable")?;
                let excluded = tagger
                    .normalization_exclusions(&sentence.text, Some(user))
                    .map_err(|e| e.to_string())?;
                let normalized = japanese::normalize_excluding(&sentence.text, &excluded)
                    .map_err(|e| e.to_string())?;
                let morphs = tagger
                    .parse_with_user(&normalized, user)
                    .map_err(|e| e.to_string())?;
                let mut analysis = japanese::analyze(&morphs).map_err(|e| e.to_string())?;
                analysis.input_text = Some(normalized);
                analysis.normalization_exclusions = excluded;
                let positions = WordPositions::from_analysis(&sentence.text, &analysis)?;
                let mut words = state::from_analysis(&analysis)?;
                preserve(&sentence.words, &mut words)?;
                sentence.words = words;
                sentence.word_positions = positions;
            }
            if let Some(pending) = sentence.ssml.as_ref() {
                ssml::apply(pending, &sentence.word_positions, &mut sentence.words)?;
            }
            sentence.ssml = None;
            if sentence.words != old_words || sentence.word_positions != old_positions {
                sentence.query = Default::default();
            }
        }
    }
    prepared.validate()?;
    Ok(prepared)
}
