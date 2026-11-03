use super::{
    JobKind, Playback, RenderProfile, Request, Shared, cache::Cache, checkpoint, engine, unit,
};
use roicepeak::{Error, Result, scalar_layers::ScalarLayers};
use std::{sync::Arc, time::Instant};

pub(super) fn render(
    request: &Request,
    session: &Playback,
    unit_index: usize,
    cache: &mut Cache,
    shared: &Shared,
    profile: &mut RenderProfile,
) -> Result<Arc<Vec<f32>>> {
    let generation = session.generation;
    checkpoint(shared, generation, JobKind::Playback)?;
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
    let dictionary_started = Instant::now();
    let words = engine::user_words(request.user_dictionary.entries())?;
    cache.dictionary(&request.config.dictionary)?;
    let dictionary = cache.dictionary.as_ref().unwrap();
    let user = dictionary.tagger.user_dictionary(&words)?;
    let mut stamps = dictionary.stamp.clone();
    stamps.extend(
        user.definition_files()
            .iter()
            .map(|source| super::cache::Stamp {
                path: source.path.clone(),
                len: source.bytes,
                modified: source.modified,
            }),
    );
    profile.dictionary_seconds += dictionary_started.elapsed().as_secs_f64();
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
    layers.resolve_with_policy(request.config.value_policy)?;
    let words = session
        .plan
        .blocks
        .iter()
        .find(|prepared| prepared.id == block.id)
        .and_then(|prepared| prepared.sentences.get(planned.sentence.index))
        .ok_or_else(|| Error("Playback sentence state is unavailable".into()))?;
    let mut spec = unit::Spec {
        kind: JobKind::Render,
        original: &block.text[planned.sentence.bytes.clone()],
        sentence: &planned.sentence,
        saved_words: &words.words,
        ssml: words.ssml.as_ref(),
        parameters: &block.parameters,
        project_parameters: &request.project.global,
        narrator_parameters: &corrections,
        layers,
        value_policy: request.config.value_policy,
        weights: &[],
        style_offset: 0.0,
        voice: Arc::clone(&validated.binding),
        dictionary: request.user_dictionary.shared(),
        dictionary_stamp: &stamps,
    };
    checkpoint(shared, generation, JobKind::Playback)?;
    let output = if let Some(output) = unit::cached(cache, &validated.stamp, &spec, profile)? {
        output
    } else {
        let voice_started = Instant::now();
        profile.voice_requests += 1;
        let lease_result = cache.voice(&validated);
        profile.voice_seconds += voice_started.elapsed().as_secs_f64();
        let lease = lease_result?;
        profile.voice_lease(&lease);
        let voice = lease.voice;
        let weights = engine::weights(
            &voice,
            binding,
            &block.parameters,
            &corrections,
            request.config.value_policy,
        )?;
        spec.weights = &weights;
        spec.style_offset = voice.style.mix(&weights)?.intonation;
        unit::perform(cache, &voice, &user, spec, profile, || {
            checkpoint(shared, generation, JobKind::Playback)
        })?
    };
    checkpoint(shared, generation, JobKind::Playback)?;
    profile.cache_state(cache);
    output
        .pcm
        .ok_or_else(|| Error("Playback unit did not produce PCM".into()))
}
