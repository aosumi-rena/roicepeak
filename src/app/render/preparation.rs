//! Worker-local immutable query cache. This module never generates acoustic noise or PCM.
use super::{
    RenderProfile, VoiceBinding,
    cache::{Cache, CachedVoice, Stamp},
};
use crate::app::dictionary::Entry;
use roicepeak::{
    Result as CoreResult,
    frontend::UserDictionary,
    japanese::{self, Analysis},
    synthesis::{Edits, PreparedQuery},
};
use std::{
    sync::{Arc, Weak},
    time::Instant,
};
const MAX_QUERIES: usize = 64;
const MAX_QUERY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, PartialEq)]
pub(super) struct QueryKey {
    pub(super) text: String,
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
    fn remember_query(&mut self, entry: PreparedUnit) {
        let bytes = entry.bytes();
        if bytes > MAX_QUERY_BYTES {
            return;
        }
        self.queries.retain(|existing| existing.key != entry.key);
        while self.queries.len() >= MAX_QUERIES
            || self
                .queries
                .iter()
                .fold(0usize, |total, entry| total.saturating_add(entry.bytes()))
                .saturating_add(bytes)
                > MAX_QUERY_BYTES
        {
            self.queries.remove(0);
        }
        self.queries.push(entry);
    }
}
pub(super) fn analyze(
    cache: &Cache,
    text: &str,
    user: &UserDictionary,
    dictionary: &Arc<Vec<Entry>>,
    stamp: &[Stamp],
    profile: &mut RenderProfile,
) -> CoreResult<Arc<Analysis>> {
    if let Some(entry) = cache.queries.iter().find(|entry| {
        entry.key.text == text
            && entry.key.dictionary.as_ref() == dictionary.as_ref()
            && entry.key.dictionary_stamp.as_slice() == stamp
    }) {
        profile.analysis_cache_hits += 1;
        return Ok(Arc::clone(&entry.raw_analysis));
    }
    let started = Instant::now();
    let result = (|| {
        let tagger = &cache.dictionary.as_ref().unwrap().tagger;
        let excluded = tagger.normalization_exclusions(text, Some(user))?;
        let normalized = japanese::normalize_excluding(text, &excluded)?;
        let mut analysis = japanese::analyze(&tagger.parse_with_user(&normalized, user)?)?;
        analysis.input_text = Some(normalized);
        analysis.normalization_exclusions = excluded;
        Ok::<_, roicepeak::Error>(analysis)
    })();
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
    cache.remember_query(PreparedUnit {
        owner: Arc::downgrade(voice),
        key: key.clone(),
        analysis: Arc::clone(analysis),
        raw_analysis: Arc::clone(raw_analysis),
        query: Arc::clone(&query),
    });
    Ok(query)
}
