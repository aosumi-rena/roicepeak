//! Project snapshots to independent cached speech synthesis.
use super::{
    JobKind, MAX_AUDIO, PreparedProject, PreparedSentence, RenderProfile, Rendered, RenderedBlock,
    Request, SAMPLE_RATE, Shared, VoiceBinding,
    cache::{Cache, CachedVoice},
    checkpoint,
};
use crate::app::{dictionary::Entry, document::Parameters, placement};
use roicepeak::{
    Error, Result as CoreResult,
    frontend::{UserPartOfSpeech, UserWord},
    graph::Budget,
    japanese::Pronunciation,
    scalar_layers::{NormalizedScalars, ScalarLayers},
};
use std::{collections::BTreeMap, sync::Arc, time::Instant};
pub(super) fn user_words(entries: &[Entry]) -> CoreResult<Vec<UserWord>> {
    entries
        .iter()
        .filter(|entry| !entry.native.opaque)
        .map(|entry| {
            // Reuse reading/nucleus validation only; selection happens in the tagger lattice.
            let reading = Pronunciation::new(&entry.word, &entry.reading, entry.accent)?;
            Ok(UserWord {
                spelling: reading.spelling().into(),
                reading: reading.reading().into(),
                accent: reading.accent(),
                part_of_speech: UserPartOfSpeech::try_from(entry.part_of_speech)?,
                priority: entry.priority,
                overwrite_accents: entry.native.overwrite_accents.clone(),
            })
        })
        .collect()
}
pub(super) fn normalized(parameters: &Parameters) -> NormalizedScalars {
    NormalizedScalars {
        speed: parameters.speed.normalized(),
        pitch: parameters.pitch.normalized(),
        pause: parameters.pause.normalized(),
        volume: parameters.volume.normalized(),
    }
}
pub(super) fn weights(
    voice: &CachedVoice,
    binding: &VoiceBinding,
    parameters: &Parameters,
    corrections: &Parameters,
    policy: roicepeak::scalar_layers::ValuePolicy,
) -> CoreResult<Vec<f32>> {
    let names = voice.manifest.synth.emotions();
    let index = |name: &str| {
        names
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| Error(format!("Unknown stylebook emotion key {name:?}")))
    };
    if let Some(main) = &binding.main_emotion {
        index(main)?;
    }
    let mut values = BTreeMap::<usize, f32>::new();
    for (row, key) in names.iter().enumerate() {
        let stored = parameters.emotions.get(key);
        if stored.is_none()
            && !binding
                .emotion_bindings
                .iter()
                .any(|control| control.key == *key)
        {
            continue;
        }
        let block = stored.copied().unwrap_or_default().normalized();
        let correction = corrections
            .emotions
            .get(key)
            .copied()
            .unwrap_or_default()
            .normalized();
        let value = correction + block;
        if !value.is_finite() {
            return Err(Error("Emotion correction overflow".into()));
        }
        values.insert(
            row,
            if policy.allow_out_of_range {
                if policy.allow_negative {
                    value
                } else {
                    value.max(0.0)
                }
            } else {
                value.clamp(if policy.allow_negative { -1.0 } else { 0.0 }, 1.0)
            },
        );
    }
    let main = binding.main_emotion.as_ref().ok_or_else(|| {
        Error("Nonzero emotions require an explicit main_emotion engine key".into())
    })?;
    let mut weights = vec![0.0f32; names.len()];
    if let Some(first) = weights.first_mut() {
        *first = 1.0;
    }
    let mut total = 0.0f32;
    for i in 0..weights.len() {
        if let Some(value) = values.get(&i) {
            weights[i] = *value;
            total += weights[i];
        }
    }
    let residual = 1.0 - total;
    weights[index(main)?] = if policy.allow_negative {
        residual
    } else {
        residual.max(0.0)
    };
    if weights.iter().any(|v| !v.is_finite()) {
        return Err(Error("Emotion mixture overflow".into()));
    }
    Ok(weights)
}

pub(super) fn budget() -> Budget {
    Budget {
        multiply_adds: 100_000_000_000,
        output_elements: 512 * 1024 * 1024,
    }
}
pub(super) enum Outcome {
    Render(Rendered),
    Prepare(PreparedProject),
    Playback(Arc<super::Playback>),
}
pub(super) fn perform(
    request: Request,
    cache: &mut Cache,
    shared: &Shared,
    generation: u64,
    kind: JobKind,
    profile: &mut RenderProfile,
    mut progress: Option<&mut super::progress::Reporter>,
) -> CoreResult<Outcome> {
    checkpoint(shared, generation, kind)?;
    let dictionary_started = Instant::now();
    let previous_dictionary = cache
        .dictionary
        .as_ref()
        .map(|dictionary| dictionary.stamp.clone());
    let dictionary_result = (|| {
        let user_words = user_words(request.user_dictionary.entries())?;
        cache.dictionary(&request.config.dictionary)?;
        cache
            .dictionary
            .as_ref()
            .unwrap()
            .tagger
            .user_dictionary(&user_words)
    })();
    profile.dictionary_seconds += dictionary_started.elapsed().as_secs_f64();
    let user_dictionary = dictionary_result?;
    profile.dictionary_cache_hit = cache
        .dictionary
        .as_ref()
        .is_some_and(|dictionary| previous_dictionary.as_ref() == Some(&dictionary.stamp));
    checkpoint(shared, generation, kind)?;
    let mut dictionary_stamp = cache.dictionary.as_ref().unwrap().stamp.clone();
    dictionary_stamp.extend(user_dictionary.definition_files().iter().map(|source| {
        super::cache::Stamp {
            path: source.path.clone(),
            len: source.bytes,
            modified: source.modified,
        }
    }));
    let dictionary_entries = request.user_dictionary.shared();
    let mut planner = placement::Planner::new(MAX_AUDIO);
    let mut placed = Vec::new();
    let mut blocks = Vec::new();
    let mut prepared_units = Vec::new();
    for block in &request.project.blocks {
        checkpoint(shared, generation, kind)?;
        if block.text.trim().is_empty() {
            let position = planner.append(block, &[]).map_err(Error)?;
            blocks.push(RenderedBlock {
                id: block.id,
                start: position.start,
                duration: 0.0,
                segments: Vec::new(),
                sentences: Vec::new(),
                timing: Vec::new(),
            });
            continue;
        }
        let units = block.sentence_ranges();
        let mut canonical = block.resolved_sentences().map_err(Error)?;
        let binding = request.config.binding(&block.narrator).map_err(Error)?;
        let validation_started = Instant::now();
        let validated = cache.validate_voice(binding)?;
        profile.voice_validation_seconds += validation_started.elapsed().as_secs_f64();
        let mut compiled = None;
        let corrections = request
            .project
            .narrator_settings
            .get(&block.narrator.identity)
            .cloned()
            .unwrap_or_default();
        let layers = ScalarLayers {
            block: normalized(&block.parameters),
            project: normalized(&request.project.global),
            narrator: normalized(&corrections),
        };
        layers.resolve_with_policy(request.config.value_policy)?;
        let parameters = block.parameters.clone();
        let mut unit_audio = Vec::with_capacity(units.len());
        let mut durations = Vec::with_capacity(units.len());
        let mut unit_ranges = Vec::with_capacity(units.len());
        let mut unit_timing = Vec::with_capacity(units.len());
        let mut segments = Vec::new();
        let mut block_samples = 0usize;
        for sentence in &units {
            checkpoint(shared, generation, kind)?;
            // Partition original text first. Normalization and token indices
            // belong exclusively to this unit and never replace source ranges.
            let original = &block.text[sentence.bytes.clone()];
            let mut spec = super::unit::Spec {
                kind: if kind == JobKind::Playback {
                    JobKind::Prepare
                } else {
                    kind
                },
                original,
                sentence,
                saved_words: &canonical[sentence.index].words,
                ssml: canonical[sentence.index].ssml.as_ref(),
                parameters: &parameters,
                project_parameters: &request.project.global,
                narrator_parameters: &corrections,
                layers,
                value_policy: request.config.value_policy,
                weights: &[],
                style_offset: 0.0,
                voice: Arc::clone(&validated.binding),
                dictionary: Arc::clone(&dictionary_entries),
                dictionary_stamp: &dictionary_stamp,
            };
            let output = if let Some(output) =
                super::unit::cached(cache, &validated.stamp, &spec, profile)?
            {
                output
            } else {
                if compiled.is_none() {
                    let voice_started = Instant::now();
                    profile.voice_requests += 1;
                    let lease_result = cache.voice(&validated);
                    profile.voice_seconds += voice_started.elapsed().as_secs_f64();
                    let lease = lease_result?;
                    profile.voice_lease(&lease);
                    let voice = lease.voice;
                    let weights = weights(
                        &voice,
                        binding,
                        &parameters,
                        &corrections,
                        request.config.value_policy,
                    )?;
                    let style_offset = voice.style.mix(&weights)?.intonation;
                    compiled = Some((voice, weights, style_offset));
                }
                let (voice, weights, style_offset) = compiled.as_ref().unwrap();
                spec.weights = weights;
                spec.style_offset = *style_offset;
                super::unit::perform(cache, voice, &user_dictionary, spec, profile, || {
                    checkpoint(shared, generation, kind)
                })?
            };
            if let Some(progress) = progress.as_deref_mut() {
                progress.complete(
                    block.id,
                    sentence,
                    output.samples,
                    output.pcm.as_ref().map(|pcm| pcm.len()),
                    &validated.stamp,
                    &dictionary_stamp,
                );
            }
            canonical[sentence.index].words = output.words;
            canonical[sentence.index].query = output.word_query;
            canonical[sentence.index].word_positions = output.word_positions;
            // The cloned result alone consumes the queue; source acceptance remains atomic.
            canonical[sentence.index].ssml = None;
            block_samples = block_samples
                .checked_add(output.samples)
                .filter(|samples| *samples <= MAX_AUDIO)
                .ok_or_else(|| {
                    Error("Block sentence timing exceeds the synthesis sample budget".into())
                })?;
            durations.push(output.samples as f64 / f64::from(SAMPLE_RATE));
            unit_timing.push(output.timing);
            let start = segments.len();
            segments.extend(output.segments);
            unit_ranges.push((sentence.clone(), start..segments.len()));
            if let Some(pcm) = output.pcm {
                unit_audio.push(pcm);
            }
            if segments.len() > 100_000 {
                return Err(Error("Block phoneme segment limit".into()));
            }
        }
        let position = planner.append(block, &durations).map_err(Error)?;
        let timing = position
            .sentences
            .iter()
            .zip(&units)
            .zip(unit_timing)
            .map(|((placed, sentence), phones)| super::SentenceTiming {
                sentence: sentence.clone(),
                start: placed.start,
                phones,
            })
            .collect();
        for (unit, (sentence, segment_range)) in position.sentences.iter().zip(unit_ranges) {
            prepared_units.push(PreparedSentence {
                block: block.id,
                sentence,
                start: unit.start,
                duration: unit.duration,
                sample_start: unit.sample_start,
                segment_range,
                model_stamp: validated.stamp.clone(),
                dictionary_stamp: dictionary_stamp.clone(),
            });
        }
        for (unit, pcm) in position.sentences.iter().zip(unit_audio) {
            placed.push((unit.sample_start, pcm));
        }
        blocks.push(RenderedBlock {
            id: block.id,
            start: position.start,
            duration: position.duration,
            segments,
            sentences: canonical,
            timing,
        });
    }
    checkpoint(shared, generation, kind)?;
    if kind != JobKind::Render {
        profile.cache_state(cache);
        return Ok(Outcome::Prepare(PreparedProject {
            config: request.config,
            source: request.project,
            user_dictionary: request.user_dictionary,
            blocks,
            units: prepared_units,
            profile: profile.clone(),
            samples: planner.sample_count().map_err(Error)?,
        }));
    }
    let mut samples = vec![0.0f32; planner.sample_count().map_err(Error)?];
    if samples.is_empty() {
        return Err(Error("Project contains no text to synthesize".into()));
    }
    for (sample_start, pcm) in placed {
        placement::mix(&mut samples, sample_start, &pcm).map_err(Error)?;
    }
    if let Some(progress) = progress.as_deref_mut() {
        progress.finish(samples.len());
    }
    profile.cache_state(cache);
    Ok(Outcome::Render(Rendered {
        pcm: Arc::new(samples),
        rate: SAMPLE_RATE,
        blocks,
        source: request.project,
        user_dictionary: request.user_dictionary,
        profile: profile.clone(),
    }))
}
