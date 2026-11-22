use super::{
    JobKind, Playback, RenderProfile, Request, Shared, cache::Cache, checkpoint, engine,
    streaming::PcmRange, unit,
};
use roicepeak::{Error, Result, scalar_layers::ScalarLayers, speech_frontend::Language};
use std::{sync::Arc, time::Instant};

fn check(session: &Playback, shared: &Shared) -> Result<()> {
    if session.cancelled() || session.failure().is_some() {
        return Err(Error("Synthesis cancelled".into()));
    }
    checkpoint(shared, session.generation, JobKind::Playback)
}

pub(super) fn prepare(
    request: &Request,
    session: &Playback,
    unit_index: usize,
    cache: &mut Cache,
    shared: &Shared,
    profile: &mut RenderProfile,
) -> Result<unit::Prepared> {
    let check = || check(session, shared);
    check()?;
    let planned = &session.plan.units[unit_index];
    let block = request
        .project
        .blocks
        .iter()
        .find(|block| block.id == planned.block)
        .ok_or_else(|| Error("Playback block is no longer available".into()))?;
    if !request.user_dictionary.same(&session.plan.user_dictionary) {
        return Err(Error(
            "Playback dictionary disagrees with its preparation".into(),
        ));
    }
    let language = Language::from_name(&block.narrator.language)
        .ok_or_else(|| Error(format!("Unsupported language {}", block.narrator.language)))?;
    let (user, stamps) =
        engine::dictionary(cache, request, language == Language::Japanese, profile)?;
    let binding = request.config.binding(&block.narrator).map_err(Error)?;
    let validation_started = Instant::now();
    let validated = cache.validate_voice(binding)?;
    profile.voice_validation_seconds += validation_started.elapsed().as_secs_f64();
    if validated.stamp != planned.model_stamp || stamps != planned.dictionary_stamp {
        return Err(Error(
            "Playback resources changed after query preparation".into(),
        ));
    }
    let corrections = request
        .project
        .narrator_settings
        .get(&block.narrator.identity)
        .cloned()
        .unwrap_or_default();
    let layers = ScalarLayers {
        block: engine::normalized(&block.parameters),
        project: engine::normalized(&request.project.global),
        narrator: engine::normalized(&corrections),
    };
    let volume_gain = layers
        .resolve_with_policy(request.config.value_policy)?
        .volume_gain;
    let words = session
        .plan
        .blocks
        .iter()
        .find(|prepared| prepared.id == block.id)
        .and_then(|prepared| prepared.sentences.get(planned.sentence.index))
        .ok_or_else(|| Error("Playback sentence state is unavailable".into()))?;
    let mut spec = unit::Spec {
        kind: JobKind::Playback,
        generation: session.generation,
        keep_prepared_queries: true,
        original: &block.text[planned.sentence.bytes.clone()],
        language,
        sentence: &planned.sentence,
        saved_words: &words.words,
        ssml: words.ssml.as_ref(),
        parameters: &block.parameters,
        project_parameters: &request.project.global,
        narrator_parameters: &corrections,
        layers,
        volume_gain,
        value_policy: request.config.value_policy,
        weights: &[],
        voice: Arc::clone(&validated.binding),
        dictionary: request.user_dictionary.shared(),
        dictionary_stamp: &stamps,
    };
    check()?;
    let prepared = if let Some(output) = unit::cached(cache, &validated.stamp, &spec, profile)? {
        unit::Prepared::Ready(output)
    } else {
        let voice_started = Instant::now();
        profile.voice_requests += 1;
        let lease_result = cache.voice(&validated);
        profile.voice_seconds += voice_started.elapsed().as_secs_f64();
        let lease = lease_result?;
        profile.voice_lease(&lease);
        let voice = lease.voice;
        session.set_seek_preroll(unit_index, voice.manifest.synth.seek_preroll_samples());
        let weights = engine::weights(
            &voice,
            binding,
            &block.parameters,
            &corrections,
            request.config.value_policy,
        )?;
        spec.weights = &weights;
        unit::prepare(cache, &voice, user.as_ref(), spec, profile, check)?
    };
    check()?;
    profile.cache_state(cache);
    Ok(prepared)
}

pub(super) fn render(
    audio: unit::Audio,
    session: &Playback,
    index: usize,
    shared: &Shared,
    profile: &mut RenderProfile,
) -> Result<unit::RenderedAudio> {
    let check = || check(session, shared);
    let mut emit = |start: usize, samples: &[f32]| {
        check()?;
        session.append(index, start, samples).map_err(Error)
    };
    audio.render(
        profile,
        check,
        Some(unit::PlaybackTarget {
            sample: session.seek_target(index),
            emit: &mut emit,
        }),
    )
}

pub(super) fn pcm(output: unit::Output) -> Result<PcmRange> {
    Ok(PcmRange {
        start: output.pcm_start,
        samples: output
            .pcm
            .ok_or_else(|| Error("Playback unit did not produce PCM".into()))?,
    })
}
