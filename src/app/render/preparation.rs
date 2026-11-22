//! Worker-local immutable query cache. This module never generates acoustic noise or PCM.
use super::{
    RenderProfile, Request, VoiceBinding,
    cache::{Cache, CachedVoice, Stamp},
};
use crate::app::dictionary::Entry;
use roicepeak::{
    Result as CoreResult,
    frontend::UserDictionary,
    japanese::Analysis,
    speech_frontend::Language,
    synthesis::{Edits, PreparedQuery},
};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, Weak},
    time::Instant,
};
const MAX_QUERIES: usize = 64;
const MAX_QUERY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, PartialEq)]
pub(super) struct QueryKey {
    pub(super) text: String,
    pub(super) language: Language,
    pub(super) model_path: std::path::PathBuf,
    pub(super) voice: Arc<VoiceBinding>,
    pub(super) model_stamp: Vec<Stamp>,
    pub(super) dictionary: Arc<Vec<Entry>>,
    pub(super) dictionary_stamp: Vec<Stamp>,
    pub(super) words: Vec<String>,
    pub(super) token_signature: Vec<u8>,
    pub(super) weights: Vec<f32>,
    pub(super) predict: Option<Vec<Vec<bool>>>,
    pub(super) accent_codes: Option<Vec<u32>>,
}
impl QueryKey {
    fn source(&self) -> (&str, &str, &Path) {
        (self.language.name(), &self.text, &self.voice.source)
    }
}
#[derive(Clone)]
pub(super) struct PreparedUnit {
    pub(super) owner: Weak<CachedVoice>,
    pub(super) key: QueryKey,
    pub(super) analysis: Arc<Analysis>,
    pub(super) raw_analysis: Arc<Analysis>,
    pub(super) query: Arc<PreparedQuery>,
}
impl PreparedUnit {
    fn bytes(&self) -> usize {
        super::cache::memory::prepared(self)
    }
}
impl Cache {
    pub(super) fn retain_project_queries(&mut self, request: &Request) {
        self.queries
            .retain(|entry| entry.key.dictionary.as_slice() == request.user_dictionary.entries());
        if self.queries.is_empty() {
            return;
        }
        let cached: BTreeSet<_> = self
            .queries
            .iter()
            .map(|entry| entry.key.source())
            .collect();
        let mut matched = BTreeSet::new();
        for block in &request.project.blocks {
            let Ok(binding) = request.config.binding(&block.narrator) else {
                continue;
            };
            for sentence in block.sentence_ranges() {
                let source = (
                    block.narrator.language.as_str(),
                    &block.text[sentence.bytes],
                    binding.source.as_path(),
                );
                if cached.contains(&source)
                    && matched.insert(source)
                    && matched.len() == cached.len()
                {
                    return;
                }
            }
        }
        self.queries
            .retain(|entry| matched.contains(&entry.key.source()));
    }

    fn remember_query(&mut self, entry: PreparedUnit, keep_prepared_queries: bool) {
        let bytes = entry.bytes();
        if bytes > MAX_QUERY_BYTES {
            return;
        }
        self.queries.retain(|existing| existing.key != entry.key);
        let mut retained = self
            .queries
            .iter()
            .fold(0usize, |total, entry| total.saturating_add(entry.bytes()));
        while self.queries.len() >= MAX_QUERIES || retained.saturating_add(bytes) > MAX_QUERY_BYTES
        {
            // Keep cached queries for the remaining sentences of a prepared render.
            if keep_prepared_queries {
                return;
            }
            retained = retained.saturating_sub(self.queries.remove(0).bytes());
        }
        self.queries.push(entry);
    }
}
pub(super) fn analyze(
    cache: &Cache,
    text: &str,
    language: Language,
    user: Option<&UserDictionary>,
    dictionary: &Arc<Vec<Entry>>,
    stamp: &[Stamp],
    profile: &mut RenderProfile,
) -> CoreResult<Arc<Analysis>> {
    if let Some(entry) = cache.queries.iter().find(|entry| {
        entry.key.text == text
            && entry.key.language == language
            && entry.key.dictionary.as_ref() == dictionary.as_ref()
            && entry.key.dictionary_stamp.as_slice() == stamp
    }) {
        profile.analysis_cache_hits += 1;
        return Ok(Arc::clone(&entry.raw_analysis));
    }
    let started = Instant::now();
    let result = language.analyze(
        text,
        cache
            .dictionary
            .as_ref()
            .zip(user)
            .map(|(entry, user)| (&entry.tagger, user)),
    );
    profile.text_analysis_seconds += started.elapsed().as_secs_f64();
    let analysis = result?;
    profile.analyzed_units += 1;
    Ok(Arc::new(analysis))
}
pub(super) fn query(
    cache: &mut Cache,
    voice: &Arc<CachedVoice>,
    key: &QueryKey,
    analysis: &Arc<Analysis>,
    raw_analysis: &Arc<Analysis>,
    edits: &Edits,
    keep_prepared_queries: bool,
    profile: &mut RenderProfile,
    check: impl Fn() -> CoreResult<()>,
) -> CoreResult<Arc<PreparedQuery>> {
    check()?;
    if key.model_path != voice.path
        || key.model_stamp != voice.stamp
        || key.voice.as_ref() != voice.binding.as_ref()
        || key.predict != edits.predict
        || key.accent_codes != edits.accent_codes
        || key.token_signature
            != crate::app::document::state::token_signature(analysis).map_err(roicepeak::Error)?
        || key.words.len() != analysis.tokens.len()
        || key
            .words
            .iter()
            .zip(&analysis.tokens)
            .any(|(word, token)| word != &token.text)
    {
        return Err(roicepeak::Error(
            "Prepared query key does not match its inputs".into(),
        ));
    }
    if let Some(entry) = cache.queries.iter().find(|entry| {
        entry.key == *key
            && entry
                .owner
                .upgrade()
                .is_some_and(|owner| Arc::ptr_eq(&owner, voice))
    }) {
        profile.query_cache_hits += 1;
        return Ok(Arc::clone(&entry.query));
    }
    profile.query_cache_misses += 1;
    let started = Instant::now();
    let mut budget = super::engine::budget();
    let result = voice.manifest.synth.prepare_edited_profiled(
        &analysis.tokens,
        &key.weights,
        edits,
        &mut budget,
    );
    profile.query_call(&result, started.elapsed());
    let query = Arc::new(result?);
    check()?;
    cache.remember_query(
        PreparedUnit {
            owner: Arc::downgrade(voice),
            key: key.clone(),
            analysis: Arc::clone(analysis),
            raw_analysis: Arc::clone(raw_analysis),
            query: Arc::clone(&query),
        },
        keep_prepared_queries,
    );
    Ok(query)
}
