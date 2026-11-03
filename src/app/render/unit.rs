//! One sentence's canonical query, controls and optional audio.
use super::{
    JobKind, MAX_AUDIO, RenderProfile, VoiceBinding,
    cache::{Cache, CachedVoice, Stamp, UnitKey},
    edits::{annotate_preview, preview, unpack},
    preparation::{self, QueryKey},
};
use crate::app::{
    dictionary::Entry,
    document::{Parameters, Segment, WordState, state},
    sentences::SentenceRef,
};
use roicepeak::{
    Error, Result as CoreResult, frontend::UserDictionary, scalar_layers::ScalarLayers,
    synthesis::BoundarySilence,
};
use std::{borrow::Cow, sync::Arc, time::Instant};
pub(super) struct Spec<'a> {
    pub(super) kind: JobKind,
    pub(super) original: &'a str,
    pub(super) sentence: &'a SentenceRef,
    pub(super) saved_words: &'a [WordState],
    pub(super) ssml: Option<&'a crate::app::document::ssml::Pending>,
    pub(super) parameters: &'a Parameters,
    pub(super) project_parameters: &'a Parameters,
    pub(super) narrator_parameters: &'a Parameters,
    pub(super) layers: ScalarLayers,
    pub(super) value_policy: roicepeak::scalar_layers::ValuePolicy,
    pub(super) weights: &'a [f32],
    pub(super) style_offset: f32,
    pub(super) voice: Arc<VoiceBinding>,
    pub(super) dictionary: Arc<Vec<Entry>>,
    pub(super) dictionary_stamp: &'a [Stamp],
}
pub(super) struct Output {
    pub(super) pcm: Option<Arc<Vec<f32>>>,
    pub(super) segments: Vec<Segment>,
    pub(super) words: Vec<WordState>,
    pub(super) word_query: state::WordQueryState,
    pub(super) word_positions: crate::app::sentence_edit::WordPositions,
    pub(super) samples: usize,
    pub(super) timing: Vec<super::PhoneTiming>,
}
impl Spec<'_> {
    fn key(&self) -> UnitKey {
        UnitKey {
            value_policy: self.value_policy,
            text: self.original.to_owned(),
            parameters: self.parameters.clone(),
            project_parameters: self.project_parameters.clone(),
            narrator_parameters: self.narrator_parameters.clone(),
            scalar_layers: self.layers,
        }
    }
}
pub(super) fn cached(
    cache: &mut Cache,
    model_stamp: &[Stamp],
    spec: &Spec<'_>,
    profile: &mut RenderProfile,
) -> CoreResult<Option<Output>> {
    if spec.ssml.is_some() || spec.saved_words.iter().any(WordState::manual) {
        return Ok(None);
    }
    let key = spec.key();
    let Some(index) = cache.baseline.iter().position(|base| {
        base.unit == key
            && base.dictionary.as_ref() == spec.dictionary.as_ref()
            && base.voice.as_deref() == Some(spec.voice.as_ref())
            && base.model_stamp.as_slice() == model_stamp
            && base.dictionary_stamp.as_slice() == spec.dictionary_stamp
    }) else {
        return Ok(None);
    };
    let base = &cache.baseline[index];
    if base.pcm.len() > MAX_AUDIO {
        return Err(Error(
            "Cached sentence exceeds the synthesis sample budget".into(),
        ));
    }
    let words = state::reconcile(spec.saved_words, &base.neutral_words).map_err(Error)?;
    if words != base.neutral_words || words.iter().any(WordState::manual) {
        return Ok(None);
    }
    let mut segments = base.segments.clone();
    for segment in &mut segments {
        segment.sentence = spec.sentence.clone();
    }
    state::project(&words, &mut segments);
    let output = Output {
        pcm: (spec.kind != JobKind::Prepare).then(|| Arc::clone(&base.pcm)),
        samples: base.pcm.len(),
        segments,
        words,
        word_query: base.word_query.clone(),
        word_positions: base.word_positions.clone(),
        timing: base.timing.clone(),
    };
    if spec.kind == JobKind::Prepare {
        profile.prepared_metadata_cache_hits += 1;
        profile.prepared_units += 1;
        profile.prepared_phones += output.segments.len();
    } else {
        profile.baseline_cache_hits += 1;
        profile.pcm_fast_path_hits += 1;
    }
    let base = cache.baseline.remove(index);
    cache.baseline.push(base);
    Ok(Some(output))
}
pub(super) fn perform(
    cache: &mut Cache,
    voice: &Arc<CachedVoice>,
    user: &UserDictionary,
    spec: Spec<'_>,
    profile: &mut RenderProfile,
    check: impl Fn() -> CoreResult<()>,
) -> CoreResult<Output> {
    check()?;
    let raw = preparation::analyze(
        cache,
        spec.original,
        user,
        &spec.dictionary,
        spec.dictionary_stamp,
        profile,
    )?;
    let raw_words = state::from_analysis(&raw).map_err(Error)?;
    let word_positions =
        crate::app::sentence_edit::WordPositions::from_analysis(spec.original, &raw)
            .map_err(Error)?;
    let authored_words: Cow<'_, [WordState]> = if let Some(pending) = spec.ssml {
        let mut words = state::reconcile(spec.saved_words, &raw_words).map_err(Error)?;
        crate::app::document::ssml::apply(pending, &word_positions, &mut words).map_err(Error)?;
        Cow::Owned(words)
    } else {
        Cow::Borrowed(spec.saved_words)
    };
    let (analysis, pronunciation) =
        state::pronunciation_query(&raw, authored_words.as_ref()).map_err(Error)?;
    let analysis = Arc::new(analysis);
    let core_parameters = voice.manifest.parameters.clone();
    let mut edits = voice.manifest.edits.clone();
    edits.boundary_silence = BoundarySilence::NativeEditorDefault;
    edits.scalar_layers = Some(spec.layers);
    edits.value_policy = spec.value_policy;
    edits.predict = Some(analysis.predict.clone());
    let mut key = QueryKey {
        text: spec.original.to_owned(),
        model_path: voice.path.clone(),
        voice: Arc::clone(&spec.voice),
        model_stamp: voice.stamp.clone(),
        dictionary: Arc::clone(&spec.dictionary),
        dictionary_stamp: spec.dictionary_stamp.to_vec(),
        words: analysis.tokens.iter().map(|t| t.text.clone()).collect(),
        token_signature: state::token_signature(&analysis).map_err(Error)?,
        weights: spec.weights.to_vec(),
        predict: edits.predict.clone(),
        accent_codes: edits.accent_codes.clone(),
    };
    let mut query =
        preparation::query(cache, voice, &key, &analysis, &raw, &edits, profile, &check)?;
    let started = Instant::now();
    let controls = voice
        .manifest
        .synth
        .controls_prepared(&query, &core_parameters, &edits);
    profile.preview_controls_seconds += started.elapsed().as_secs_f64();
    let mut phones = preview(
        &query,
        &controls?,
        &analysis,
        spec.style_offset,
        &edits,
        core_parameters.invert_intonation,
        spec.sentence,
    )?;
    let predicted = state::projected_codes(&pronunciation, &phones.segments);
    let fresh = predicted.clone();
    let mut words = state::reconcile(authored_words.as_ref(), &fresh).map_err(Error)?;
    let mut saved = phones.segments.clone();
    state::project(&words, &mut saved);
    let changed = annotate_preview(&phones, &saved, &mut edits)?;
    let authored = words.iter().any(WordState::manual);
    if changed {
        if key.accent_codes != edits.accent_codes {
            key.accent_codes = edits.accent_codes.clone();
            query =
                preparation::query(cache, voice, &key, &analysis, &raw, &edits, profile, &check)?;
        }
        let started = Instant::now();
        let controls = voice
            .manifest
            .synth
            .controls_prepared(&query, &core_parameters, &edits);
        profile.preview_controls_seconds += started.elapsed().as_secs_f64();
        phones = preview(
            &query,
            &controls?,
            &analysis,
            spec.style_offset,
            &edits,
            core_parameters.invert_intonation,
            spec.sentence,
        )?;
        words = state::reconcile(
            &words,
            &state::projected_codes(&pronunciation, &phones.segments),
        )
        .map_err(Error)?;
    }
    let word_query = state::WordQueryState {
        text: spec.original.to_owned(),
        raw: raw_words,
        pronunciation,
        predicted,
        controlled: state::projected_codes(&words, &phones.segments),
    };
    state::project(&words, &mut phones.segments);
    check()?;
    if spec.kind == JobKind::Prepare {
        profile.prepared_units += 1;
        profile.prepared_phones += phones.segments.len();
        return Ok(Output {
            pcm: None,
            samples: phones.samples,
            segments: phones.segments,
            words,
            word_query,
            word_positions,
            timing: phones.timing,
        });
    }
    let unit = spec.key();
    let existing = if authored {
        None
    } else {
        cache
            .baseline
            .iter()
            .find(|base| {
                base.unit == unit
                    && base.dictionary.as_ref() == spec.dictionary.as_ref()
                    && base.voice.as_deref() == Some(spec.voice.as_ref())
                    && base.model_stamp == key.model_stamp
                    && base.dictionary_stamp.as_slice() == spec.dictionary_stamp
                    && base.weights.as_slice() == spec.weights
            })
            .cloned()
    };
    if let Some(existing) = existing {
        profile.baseline_cache_hits += 1;
        return Ok(Output {
            samples: existing.pcm.len(),
            pcm: Some(existing.pcm),
            segments: phones.segments,
            words,
            word_query,
            word_positions,
            timing: phones.timing,
        });
    }
    profile.baseline_cache_misses += 1;
    let started = Instant::now();
    let rendered = voice.manifest.synth.render_prepared_profiled(
        &query,
        &core_parameters,
        &edits,
        &mut super::engine::budget(),
    );
    profile.core_call(authored, &rendered, started.elapsed());
    let mut base = unpack(
        rendered?,
        &analysis,
        spec.style_offset,
        &edits,
        core_parameters.invert_intonation,
        spec.sentence,
    )?;
    base.unit = unit;
    base.dictionary = Arc::clone(&spec.dictionary);
    base.voice = Some(Arc::clone(&spec.voice));
    base.model_stamp = key.model_stamp.clone();
    base.weights = spec.weights.to_vec();
    base.dictionary_stamp = spec.dictionary_stamp.to_vec();
    if authored {
        state::project(&words, &mut base.segments);
    } else {
        base.neutral_words = fresh;
        base.word_query = word_query.clone();
        base.word_positions = word_positions.clone();
        cache.remember(base.clone());
    }
    check()?;
    if base.pcm.len() > MAX_AUDIO {
        return Err(Error("Sentence exceeds the synthesis sample budget".into()));
    }
    Ok(Output {
        samples: base.pcm.len(),
        pcm: Some(base.pcm),
        segments: base.segments,
        words,
        word_query,
        word_positions,
        timing: base.timing,
    })
}
