//! One sentence's canonical query, controls and optional audio.
#[path = "unit_batch.rs"]
pub(super) mod batch;

use super::{
    JobKind, MAX_AUDIO, RenderProfile, VoiceBinding,
    cache::{AuthoredControls, Cache, CachedVoice, Stamp, UnitKey},
    edits::{PhonePreview, annotate_preview, preview, unpack},
    preparation::{self, QueryKey},
};
use crate::app::{
    dictionary::Entry,
    document::{ControlValue, Parameters, Segment, WordState, state},
    sentence_edit::WordPositions,
    sentences::SentenceRef,
};
use roicepeak::{
    Error, Result as CoreResult,
    frontend::UserDictionary,
    japanese::Analysis,
    scalar_layers::{NormalizedScalars, ScalarLayers},
    speech_frontend::Language,
    synthesis::{BoundarySilence, Edits, PreparedQuery},
};
use std::{borrow::Cow, sync::Arc, time::Instant};
pub(super) struct Spec<'a> {
    pub(super) kind: JobKind,
    pub(super) generation: u64,
    pub(super) keep_prepared_queries: bool,
    pub(super) original: &'a str,
    pub(super) language: Language,
    pub(super) sentence: &'a SentenceRef,
    pub(super) saved_words: &'a [WordState],
    pub(super) ssml: Option<&'a crate::app::document::ssml::Pending>,
    pub(super) parameters: &'a Parameters,
    pub(super) project_parameters: &'a Parameters,
    pub(super) narrator_parameters: &'a Parameters,
    pub(super) layers: ScalarLayers,
    pub(super) volume_gain: f64,
    pub(super) value_policy: roicepeak::scalar_layers::ValuePolicy,
    pub(super) weights: &'a [f32],
    pub(super) voice: Arc<VoiceBinding>,
    pub(super) dictionary: Arc<Vec<Entry>>,
    pub(super) dictionary_stamp: &'a [Stamp],
}
pub(super) struct Output {
    pub(super) pcm: Option<Arc<Vec<f32>>>,
    pub(super) pcm_start: usize,
    pub(super) segments: Vec<Segment>,
    pub(super) words: Vec<WordState>,
    pub(super) word_query: state::WordQueryState,
    pub(super) word_positions: crate::app::sentence_edit::WordPositions,
    pub(super) samples: usize,
    pub(super) timing: Vec<super::PhoneTiming>,
}
pub(super) struct PlaybackTarget<'a> {
    pub(super) sample: usize,
    pub(super) emit: &'a mut dyn FnMut(usize, &[f32]) -> CoreResult<()>,
}
pub(super) enum Prepared {
    Ready(Output),
    Audio(Box<Audio>),
}
pub(super) struct Audio {
    voice: Arc<CachedVoice>,
    query: Arc<PreparedQuery>,
    analysis: Arc<Analysis>,
    edits: Edits,
    sentence: SentenceRef,
    unit: UnitKey,
    key: QueryKey,
    volume_gain: f64,
    phones: PhonePreview,
    authored: bool,
    words: Vec<WordState>,
    fresh: Vec<WordState>,
    word_query: state::WordQueryState,
    word_positions: WordPositions,
}
pub(super) struct RenderedAudio {
    pub(super) output: Output,
    pub(super) baseline: Option<super::cache::Baseline>,
}
impl Spec<'_> {
    fn synthesis_layers(&self) -> ScalarLayers {
        let unity = |layer| NormalizedScalars {
            volume: 1.0,
            ..layer
        };
        ScalarLayers {
            block: unity(self.layers.block),
            project: unity(self.layers.project),
            narrator: unity(self.layers.narrator),
        }
    }

    fn key(&self) -> UnitKey {
        let unity = |parameters: &Parameters| Parameters {
            volume: ControlValue::from_percent(100.0),
            ..parameters.clone()
        };
        UnitKey {
            value_policy: self.value_policy,
            text: self.original.to_owned(),
            language: self.language,
            parameters: unity(self.parameters),
            project_parameters: unity(self.project_parameters),
            narrator_parameters: unity(self.narrator_parameters),
            scalar_layers: self.synthesis_layers(),
        }
    }
}

fn apply_volume(samples: &mut [f32], gain: f64) -> CoreResult<()> {
    for sample in samples {
        *sample = (f64::from(*sample) * gain) as f32;
        if !sample.is_finite() {
            return Err(Error("Non-finite inference result".into()));
        }
    }
    Ok(())
}

fn volume_pcm(mut pcm: Arc<Vec<f32>>, gain: f64) -> CoreResult<Arc<Vec<f32>>> {
    if gain == 0.0 {
        return Ok(Arc::new(vec![0.0; pcm.len()]));
    }
    if gain != 1.0 {
        apply_volume(Arc::make_mut(&mut pcm).as_mut_slice(), gain)?;
    }
    Ok(pcm)
}
pub(super) fn cached(
    cache: &mut Cache,
    model_stamp: &[Stamp],
    spec: &Spec<'_>,
    profile: &mut RenderProfile,
) -> CoreResult<Option<Output>> {
    if spec.kind != JobKind::Prepare {
        cache.visit_audio(
            spec.original,
            spec.language,
            &spec.voice.source,
            spec.generation,
        );
    }
    if spec.ssml.is_some() || spec.saved_words.iter().any(WordState::manual) {
        return Ok(None);
    }
    let key = spec.key();
    let Some(index) = cache.baseline.iter().position(|base| {
        base.unit == key
            && base.authored.is_none()
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
    let pcm = if spec.kind == JobKind::Prepare {
        None
    } else {
        Some(volume_pcm(Arc::clone(&base.pcm), spec.volume_gain)?)
    };
    let output = Output {
        pcm,
        pcm_start: 0,
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
pub(super) fn prepare(
    cache: &mut Cache,
    voice: &Arc<CachedVoice>,
    user: Option<&UserDictionary>,
    spec: Spec<'_>,
    profile: &mut RenderProfile,
    check: impl Fn() -> CoreResult<()>,
) -> CoreResult<Prepared> {
    check()?;
    let raw = preparation::analyze(
        cache,
        spec.original,
        spec.language,
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
    edits.scalar_layers = Some(spec.synthesis_layers());
    edits.value_policy = spec.value_policy;
    edits.predict = Some(analysis.predict.clone());
    let mut key = QueryKey {
        text: spec.original.to_owned(),
        language: spec.language,
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
    let mut query = preparation::query(
        cache,
        voice,
        &key,
        &analysis,
        &raw,
        &edits,
        spec.keep_prepared_queries,
        profile,
        &check,
    )?;
    let started = Instant::now();
    let controls = voice
        .manifest
        .synth
        .controls_prepared(&query, &core_parameters, &edits);
    profile.preview_controls_seconds += started.elapsed().as_secs_f64();
    let mut phones = preview(&query, &controls?, &analysis, &edits, spec.sentence)?;
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
            query = preparation::query(
                cache,
                voice,
                &key,
                &analysis,
                &raw,
                &edits,
                spec.keep_prepared_queries,
                profile,
                &check,
            )?;
        }
        let started = Instant::now();
        let controls = voice
            .manifest
            .synth
            .controls_prepared(&query, &core_parameters, &edits);
        profile.preview_controls_seconds += started.elapsed().as_secs_f64();
        phones = preview(&query, &controls?, &analysis, &edits, spec.sentence)?;
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
    let preparing = spec.kind == JobKind::Prepare;
    let muted = spec.volume_gain == 0.0;
    if preparing || muted {
        let pcm = if preparing {
            profile.prepared_units += 1;
            profile.prepared_phones += phones.segments.len();
            None
        } else {
            if phones.samples > MAX_AUDIO {
                return Err(Error("Sentence exceeds the synthesis sample budget".into()));
            }
            Some(Arc::new(vec![0.0; phones.samples]))
        };
        return Ok(Prepared::Ready(Output {
            pcm,
            pcm_start: 0,
            samples: phones.samples,
            segments: phones.segments,
            words,
            word_query,
            word_positions,
            timing: phones.timing,
        }));
    }
    let unit = spec.key();
    let existing = cache.baseline.iter().position(|base| {
        base.unit == unit
            && base.dictionary.as_ref() == spec.dictionary.as_ref()
            && base.voice.as_deref() == Some(spec.voice.as_ref())
            && base.model_stamp == key.model_stamp
            && base.dictionary_stamp.as_slice() == spec.dictionary_stamp
            && base.weights.as_slice() == spec.weights
            && base.pcm.len() == phones.samples
            && match &base.authored {
                Some(previous) => {
                    authored
                        && previous.token_signature == key.token_signature
                        && previous.edits == edits
                }
                None => !authored,
            }
    });
    if let Some(index) = existing {
        let base = cache.baseline.remove(index);
        let pcm = volume_pcm(Arc::clone(&base.pcm), spec.volume_gain)?;
        cache.baseline.push(base);
        profile.baseline_cache_hits += 1;
        return Ok(Prepared::Ready(Output {
            samples: phones.samples,
            pcm: Some(pcm),
            pcm_start: 0,
            segments: phones.segments,
            words,
            word_query,
            word_positions,
            timing: phones.timing,
        }));
    }
    profile.baseline_cache_misses += 1;
    Ok(Prepared::Audio(Box::new(Audio {
        voice: Arc::clone(voice),
        query,
        analysis,
        edits,
        sentence: spec.sentence.clone(),
        unit,
        key,
        volume_gain: spec.volume_gain,
        phones,
        authored,
        words,
        fresh,
        word_query,
        word_positions,
    })))
}
impl Audio {
    pub(super) fn render(
        self,
        profile: &mut RenderProfile,
        check: impl Fn() -> CoreResult<()>,
        target: Option<PlaybackTarget<'_>>,
    ) -> CoreResult<RenderedAudio> {
        check()?;
        let Audio {
            voice,
            query,
            analysis,
            edits,
            sentence,
            unit,
            key,
            volume_gain,
            phones,
            authored,
            words,
            fresh,
            word_query,
            word_positions,
        } = self;
        let core_parameters = &voice.manifest.parameters;
        let started = Instant::now();
        let mut budget = super::engine::render_budget(phones.samples);
        let mut pcm_start = 0;
        let rendered = if let Some(target) = target {
            let mut scaled = Vec::new();
            let mut emit = |start, samples: &[f32]| {
                if volume_gain == 1.0 {
                    return (target.emit)(start, samples);
                }
                scaled.clear();
                scaled.extend_from_slice(samples);
                apply_volume(&mut scaled, volume_gain)?;
                (target.emit)(start, &scaled)
            };
            voice
                .manifest
                .synth
                .render_prepared_profiled_streamed_at(
                    &query,
                    core_parameters,
                    &edits,
                    &mut budget,
                    target.sample,
                    &mut emit,
                )
                .map(|(start, render)| {
                    pcm_start = start;
                    render
                })
        } else {
            voice.manifest.synth.render_prepared_profiled(
                &query,
                core_parameters,
                &edits,
                &mut budget,
            )
        };
        profile.core_call(authored, &rendered, started.elapsed());
        let mut rendered = rendered?;
        if pcm_start != 0 {
            let pcm = rendered
                .stages
                .remove("pcm")
                .ok_or_else(|| Error("Missing playback PCM".into()))?;
            if pcm.cols() != 1 || pcm_start.checked_add(pcm.rows()) != Some(phones.samples) {
                return Err(Error(
                    "Seek PCM length disagrees with sentence timing".into(),
                ));
            }
            check()?;
            return Ok(RenderedAudio {
                output: Output {
                    pcm: Some(volume_pcm(Arc::new(pcm.values().to_vec()), volume_gain)?),
                    pcm_start,
                    samples: phones.samples,
                    segments: phones.segments,
                    words,
                    word_query,
                    word_positions,
                    timing: phones.timing,
                },
                baseline: None,
            });
        }
        let mut base = unpack(rendered, unit, &analysis, &edits, &sentence)?;
        base.dictionary = key.dictionary;
        base.voice = Some(key.voice);
        base.model_stamp = key.model_stamp;
        base.weights = key.weights;
        base.dictionary_stamp = key.dictionary_stamp;
        if authored {
            base.authored = Some(Arc::new(AuthoredControls {
                token_signature: key.token_signature,
                edits,
            }));
            state::project(&words, &mut base.segments);
        } else {
            base.neutral_words = fresh;
            base.word_query = word_query.clone();
            base.word_positions = word_positions.clone();
        }
        check()?;
        if base.pcm.len() > MAX_AUDIO {
            return Err(Error("Sentence exceeds the synthesis sample budget".into()));
        }
        let baseline = Some(base.clone());
        let samples = base.pcm.len();
        let pcm = volume_pcm(base.pcm, volume_gain)?;
        Ok(RenderedAudio {
            baseline,
            output: Output {
                samples,
                pcm: Some(pcm),
                pcm_start: 0,
                segments: base.segments,
                words,
                word_query,
                word_positions,
                timing: base.timing,
            },
        })
    }
}
