//! Project snapshots to independent cached speech synthesis.
use super::{
    JobKind, MAX_AUDIO, PreparedProject, PreparedSentence, RenderProfile, Rendered, RenderedBlock,
    Request, SAMPLE_RATE, Shared, VoiceBinding,
    cache::{Cache, CachedVoice, Stamp},
    checkpoint,
};
use crate::app::{
    dictionary::Entry,
    document::{Parameters, Project},
    placement,
};
use roicepeak::{
    Error, Result as CoreResult,
    frontend::{UserDictionary, UserPartOfSpeech, UserWord},
    graph::Budget,
    japanese::Pronunciation,
    scalar_layers::{NormalizedScalars, ScalarLayers},
    speech_frontend::Language,
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

pub(super) fn render_budget(samples: usize) -> Budget {
    let intervals = samples.div_ceil(10 * SAMPLE_RATE as usize).max(1);
    let mut budget = budget();
    budget.multiply_adds *= intervals;
    budget.output_elements *= intervals;
    budget
}
pub(super) enum Outcome {
    Render(Rendered),
    Prepare(PreparedProject),
    Playback(Arc<super::Playback>),
}

fn uses_japanese_dictionary(project: &Project) -> bool {
    project.blocks.iter().any(|block| {
        block.narrator.language == Language::Japanese.name() && block.has_synthesis_units()
    })
}

pub(super) fn dictionary(
    cache: &mut Cache,
    request: &Request,
    required: bool,
    profile: &mut RenderProfile,
) -> CoreResult<(Option<UserDictionary>, Vec<Stamp>)> {
    if !required {
        return Ok((None, Vec::new()));
    }
    let path =
        request.config.dictionary.as_deref().ok_or_else(|| {
            Error("Select a Japanese pronunciation dictionary in settings".into())
        })?;
    let dictionary_started = Instant::now();
    let previous_dictionary = cache
        .dictionary
        .as_ref()
        .map(|dictionary| dictionary.stamp.clone());
    let dictionary_result = (|| {
        let user_words = user_words(request.user_dictionary.entries())?;
        cache.dictionary(path)?;
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
    let mut dictionary_stamp = cache.dictionary.as_ref().unwrap().stamp.clone();
    dictionary_stamp.extend(
        user_dictionary
            .definition_files()
            .iter()
            .map(|source| Stamp {
                path: source.path.clone(),
                len: source.bytes,
                modified: source.modified,
            }),
    );
    Ok((Some(user_dictionary), dictionary_stamp))
}

pub(super) fn reusable_plan(
    plan: &PreparedProject,
    request: &Request,
    cache: &mut Cache,
    shared: &Shared,
    generation: u64,
    profile: &mut RenderProfile,
) -> CoreResult<bool> {
    if !plan.matches(&request.project, &request.config, &request.user_dictionary) {
        return Ok(false);
    }
    checkpoint(shared, generation, JobKind::Playback)?;
    let (_, dictionary_stamp) = dictionary(
        cache,
        request,
        uses_japanese_dictionary(&request.project),
        profile,
    )?;
    let narrators: BTreeMap<_, _> = request
        .project
        .blocks
        .iter()
        .map(|block| (block.id, &block.narrator))
        .collect();
    let mut stamps = BTreeMap::new();
    for unit in &plan.units {
        checkpoint(shared, generation, JobKind::Playback)?;
        let narrator = narrators
            .get(&unit.block)
            .ok_or_else(|| Error("Unknown prepared block".into()))?;
        let expected_dictionary = if narrator.language == Language::Japanese.name() {
            dictionary_stamp.as_slice()
        } else {
            &[]
        };
        if unit.dictionary_stamp.as_slice() != expected_dictionary {
            return Ok(false);
        }
        let stamp = match stamps.entry(*narrator) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let binding = request.config.binding(narrator).map_err(Error)?;
                let start = Instant::now();
                let validated = cache.validate_voice(binding);
                profile.voice_validation_seconds += start.elapsed().as_secs_f64();
                entry.insert(validated?.stamp)
            }
        };
        if *stamp != unit.model_stamp {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn perform(
    request: Arc<Request>,
    cache: &mut Cache,
    shared: &Shared,
    generation: u64,
    kind: JobKind,
    profile: &mut RenderProfile,
    mut progress: Option<&mut super::progress::Reporter>,
) -> CoreResult<Outcome> {
    checkpoint(shared, generation, kind)?;
    let (user_dictionary, dictionary_stamp) = dictionary(
        cache,
        &request,
        uses_japanese_dictionary(&request.project),
        profile,
    )?;
    checkpoint(shared, generation, kind)?;
    let keep_prepared_queries = kind != JobKind::Render || progress.is_some();
    let render_pairs = kind == JobKind::Render
        && std::thread::available_parallelism().is_ok_and(|workers| workers.get() >= 4);
    if kind != JobKind::Render {
        cache.retain_project_queries(&request);
    }
    let dictionary_entries = request.user_dictionary.shared();
    let mut planner = placement::Planner::new(MAX_AUDIO);
    let mut placed = Vec::new();
    let mut blocks = Vec::new();
    let mut prepared_units = Vec::new();
    for block in &request.project.blocks {
        checkpoint(shared, generation, kind)?;
        if !block.has_synthesis_units() {
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
        let language = Language::from_name(&block.narrator.language)
            .ok_or_else(|| Error(format!("Unsupported language {}", block.narrator.language)))?;
        let (user, dictionary_stamp) = if language == Language::Japanese {
            (user_dictionary.as_ref(), dictionary_stamp.as_slice())
        } else {
            (None, &[][..])
        };
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
        let volume_gain = layers
            .resolve_with_policy(request.config.value_policy)?
            .volume_gain;
        let parameters = block.parameters.clone();
        let mut unit_audio = Vec::with_capacity(units.len());
        let mut durations = Vec::with_capacity(units.len());
        let mut unit_ranges = Vec::with_capacity(units.len());
        let mut unit_timing = Vec::with_capacity(units.len());
        let mut segments = Vec::new();
        let mut block_samples = 0usize;
        let check = || checkpoint(shared, generation, kind);
        let mut remaining = units.as_slice();
        while let Some(first) = remaining.first() {
            let pair = render_pairs
                && remaining.get(1).is_some_and(|second| {
                    block.text[first.bytes.clone()] != block.text[second.bytes.clone()]
                });
            let (batch, rest) = remaining.split_at(1 + usize::from(pair));
            remaining = rest;
            let mut prepared = Vec::with_capacity(batch.len());
            for sentence in batch {
                check()?;
                let mut spec = super::unit::Spec {
                    kind: if kind == JobKind::Playback {
                        JobKind::Prepare
                    } else {
                        kind
                    },
                    generation,
                    keep_prepared_queries,
                    original: &block.text[sentence.bytes.clone()],
                    language,
                    sentence,
                    saved_words: &canonical[sentence.index].words,
                    ssml: canonical[sentence.index].ssml.as_ref(),
                    parameters: &parameters,
                    project_parameters: &request.project.global,
                    narrator_parameters: &corrections,
                    layers,
                    volume_gain,
                    value_policy: request.config.value_policy,
                    weights: &[],
                    voice: Arc::clone(&validated.binding),
                    dictionary: Arc::clone(&dictionary_entries),
                    dictionary_stamp,
                };
                if let Some(output) = super::unit::cached(cache, &validated.stamp, &spec, profile)?
                {
                    prepared.push(super::unit::Prepared::Ready(output));
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
                        compiled = Some((voice, weights));
                    }
                    let (voice, weights) = compiled.as_ref().unwrap();
                    spec.weights = weights;
                    prepared.push(super::unit::prepare(
                        cache, voice, user, spec, profile, &check,
                    )?);
                }
            }
            let outputs = super::unit::batch::finish(prepared, cache, profile, generation, &check)?;
            for (sentence, output) in batch.iter().zip(outputs) {
                if let Some(progress) = progress.as_deref_mut() {
                    progress.complete(
                        block.id,
                        sentence,
                        output.samples,
                        output.pcm.as_ref().map(|pcm| pcm.len()),
                        &validated.stamp,
                        dictionary_stamp,
                    );
                }
                canonical[sentence.index].words = output.words;
                canonical[sentence.index].query = output.word_query;
                canonical[sentence.index].word_positions = output.word_positions;
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
                dictionary_stamp: dictionary_stamp.to_vec(),
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
    let Request {
        config,
        project,
        user_dictionary,
    } = Arc::unwrap_or_clone(request);
    if kind != JobKind::Render {
        profile.cache_state(cache);
        return Ok(Outcome::Prepare(PreparedProject {
            config,
            source: project,
            user_dictionary,
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
        source: project,
        user_dictionary,
        profile: profile.clone(),
    }))
}
