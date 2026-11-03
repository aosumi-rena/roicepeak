//! Conservative retained charges; transient compilation/render buffers are separate.
//! Shared allocations may be charged repeatedly. These estimates are not RSS.
use super::{Baseline, CachedVoice, Stamp};
use crate::app::{
    dictionary::Entry,
    document::{Parameters, WordState},
    render::{EmotionBinding, VoiceBinding, preparation::PreparedUnit},
};
use roicepeak::{japanese::Analysis, phonemes::Token};
use std::{mem::size_of, path::PathBuf};

fn sum(values: impl IntoIterator<Item = usize>) -> usize {
    values.into_iter().fold(0, usize::saturating_add)
}
fn allocation(bytes: usize) -> usize {
    if bytes == 0 {
        0
    } else {
        bytes.saturating_add(64)
    }
}
fn path(value: &PathBuf) -> usize {
    allocation(value.capacity())
}
fn string(value: &String) -> usize {
    allocation(value.capacity())
}
fn vector<T>(value: &Vec<T>) -> usize {
    allocation(value.capacity().saturating_mul(size_of::<T>()))
}
fn nested<T>(value: &Vec<T>, heap: impl Fn(&T) -> usize) -> usize {
    sum(std::iter::once(vector(value)).chain(value.iter().map(heap)))
}
fn arc<T>(heap: usize) -> usize {
    allocation(size_of::<T>().saturating_add(2 * size_of::<usize>())).saturating_add(heap)
}
fn stamps(values: &Vec<Stamp>) -> usize {
    nested(values, |value| path(&value.path))
}
fn parameters(value: &Parameters) -> usize {
    let node = sum([
        size_of::<(String, crate::app::document::ControlValue)>().saturating_mul(11),
        size_of::<usize>().saturating_mul(16),
    ]);
    sum([
        value.emotions.len().max(1).saturating_mul(allocation(node)),
        sum(value.emotions.keys().map(string)),
    ])
}
fn words(value: &Vec<WordState>) -> usize {
    nested(value, |word| {
        sum([
            string(&word.surface),
            nested(&word.morae, |mora| {
                sum([
                    string(&mora.reading),
                    nested(&mora.phones, |phone| string(&phone.symbol)),
                ])
            }),
        ])
    })
}
fn dictionary(value: &Vec<Entry>) -> usize {
    arc::<Vec<Entry>>(nested(value, |entry| entry.estimated_heap_bytes()))
}
fn token(value: &Token) -> usize {
    sum([
        string(&value.text),
        nested(&value.syllables, |syllable| {
            sum([
                string(&syllable.reading),
                nested(&syllable.phones, |phone| string(&phone.symbol)),
            ])
        }),
    ])
}
fn analysis(value: &Analysis) -> usize {
    arc::<Analysis>(sum([
        nested(&value.tokens, token),
        nested(&value.predict, vector),
        vector(&value.input_ranges),
        value.input_text.as_ref().map_or(0, string),
        // Conservatively charge BTreeSet nodes and allocation overhead.
        value.normalization_exclusions.len().saturating_mul(128),
    ]))
}

pub(in crate::app::render) fn binding(value: &VoiceBinding) -> usize {
    arc::<VoiceBinding>(sum([
        path(&value.source),
        arc::<Vec<u8>>(vector(&value.source_bytes)),
        nested(&value.metadata_files, |(p, b)| sum([path(p), vector(b)])),
        value.resources.estimated_bytes(),
        value.main_emotion.as_ref().map_or(0, string),
        nested(&value.emotion_bindings, |value: &EmotionBinding| {
            sum([string(&value.label), string(&value.key)])
        }),
    ]))
}

pub(in crate::app::render) fn prepared(value: &PreparedUnit) -> usize {
    let key = &value.key;
    sum([
        size_of::<PreparedUnit>(),
        // Weak references can retain an expired owner's allocation after its payload drops.
        arc::<CachedVoice>(0),
        binding(&key.voice),
        dictionary(&key.dictionary),
        analysis(&value.analysis),
        analysis(&value.raw_analysis),
        value
            .query
            .estimated_bytes()
            .saturating_add(2 * size_of::<usize>())
            .saturating_add(64),
        string(&key.text),
        path(&key.model_path),
        stamps(&key.model_stamp),
        stamps(&key.dictionary_stamp),
        nested(&key.words, string),
        vector(&key.token_signature),
        vector(&key.weights),
        key.predict.as_ref().map_or(0, |rows| nested(rows, vector)),
        key.accent_codes.as_ref().map_or(0, vector),
    ])
}

impl CachedVoice {
    pub(super) fn estimated_bytes(&self) -> usize {
        arc::<Self>(sum([
            binding(&self.binding),
            path(&self.path),
            stamps(&self.stamp),
            self.manifest.estimated_bytes(),
            self.style.estimated_bytes(),
        ]))
    }
}
impl Baseline {
    pub(super) fn estimated_bytes(&self) -> usize {
        sum([
            size_of::<Self>().saturating_add(64),
            string(&self.unit.text),
            parameters(&self.unit.parameters),
            parameters(&self.unit.project_parameters),
            parameters(&self.unit.narrator_parameters),
            dictionary(&self.dictionary),
            self.voice.as_ref().map_or(0, |voice| binding(voice)),
            stamps(&self.model_stamp),
            stamps(&self.dictionary_stamp),
            vector(&self.weights),
            arc::<Vec<f32>>(vector(&self.pcm)),
            nested(&self.segments, |segment| {
                sum([
                    string(&segment.symbol),
                    string(&segment.reading),
                    string(&segment.token_text),
                ])
            }),
            vector(&self.refs),
            vector(&self.nucleus),
            vector(&self.codes),
            nested(&self.timing, |phone| string(&phone.symbol)),
            words(&self.neutral_words),
            string(&self.word_query.text),
            string(&self.word_positions.source),
            vector(&self.word_positions.ranges),
            sum(self.word_query.snapshots().into_iter().map(words)),
        ])
    }
}
