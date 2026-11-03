//! Timing, user curves and explicit lane edits from actual inference stages.
use super::{
    MAX_AUDIO, PhoneTiming, SAMPLE_RATE,
    cache::{Baseline, UnitKey},
};
use crate::app::{
    document::{
        Parameters, Segment,
        state::{DurationMode, DurationState},
    },
    sentences::SentenceRef,
};
use roicepeak::{
    Error, Result as CoreResult,
    japanese::Analysis,
    phonemes::{Encoded, HIGH},
    synthesis::{Controls, DurationEdit, Edits, PreparedQuery, Render},
    tensor::Tensor,
};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone)]
pub(super) struct PhonePreview {
    pub(super) segments: Vec<Segment>,
    pub(super) refs: Vec<[usize; 5]>,
    pub(super) nucleus: Vec<bool>,
    pub(super) codes: Vec<u32>,
    pub(super) samples: usize,
    pub(super) timing: Vec<PhoneTiming>,
}
fn phone_preview(
    encoded: &Encoded,
    prosody: &Tensor,
    controls: &Tensor,
    raw_durations: &[f32],
    raw_intonation: &[f32],
    query_timing: &[roicepeak::synthesis::PhoneTiming],
    analysis: &Analysis,
    style_offset: f32,
    edits: &Edits,
    invert: bool,
    sentence: &SentenceRef,
) -> CoreResult<PhonePreview> {
    let _ = (style_offset, invert);
    if raw_durations.len() != encoded.references.len()
        || raw_durations.iter().any(|v| !v.is_finite() || *v < 0.0)
        || raw_intonation.iter().any(|v| !v.is_finite())
        || encoded
            .references
            .iter()
            .any(|r| r[3] >= raw_intonation.len())
    {
        return Err(Error(
            "Invalid raw query timing or intonation metadata".into(),
        ));
    }
    let codes = (0..prosody.rows())
        .map(|i| prosody.row(i)[0] as u32)
        .collect::<Vec<_>>();
    let mut high = BTreeMap::new();
    for (i, reference) in encoded.references.iter().enumerate() {
        if encoded.nucleus[i] {
            high.entry(reference[3]).or_insert(codes[i] == HIGH);
        }
    }
    let mut total = 0usize;
    let mut segments = Vec::new();
    for (i, reference) in encoded.references.iter().enumerate() {
        let count = controls.row(i)[2];
        if !count.is_finite() || count < 0.0 || count.fract() != 0.0 {
            return Err(Error("Nonintegral control frame count".into()));
        }
        let samples = (count as usize)
            .checked_mul(240)
            .ok_or_else(|| Error("Phone sample count overflow".into()))?;
        total = total
            .checked_add(samples)
            .ok_or_else(|| Error("Block sample count overflow".into()))?;
        let token = &analysis.tokens[reference[0]];
        let syllable = &token.syllables[reference[1]];
        segments.push(Segment {
            sentence: sentence.clone(),
            symbol: encoded.symbols[i].clone(),
            reading: syllable.reading.clone(),
            duration: samples as f32 / SAMPLE_RATE as f32,
            intonation: edits
                .intonation_delta
                .get(&reference[3])
                .copied()
                .unwrap_or(0.0)
                + raw_intonation[reference[3]],
            raw_query_intonation: raw_intonation[reference[3]],
            raw_query_duration_seconds: f64::from(raw_durations[i]),
            high: high.get(&reference[3]).copied().unwrap_or(false),
            accent_code: codes[i],
            accent_edited: false,
            intonation_delta: edits
                .intonation_delta
                .get(&reference[3])
                .copied()
                .unwrap_or(0.0),
            token: reference[0],
            mora: reference[1],
            phone: reference[2],
            nucleus: encoded.nucleus[i],
            token_text: token.text.clone(),
            duration_edit: DurationState::default(),
        });
    }
    if total > MAX_AUDIO {
        return Err(Error("Prepared phone timing exceeds sample budget".into()));
    }
    if query_timing.len() != encoded.references.len().saturating_sub(2) {
        return Err(Error(
            "Query phone timing dimensions do not match encoded interior phones".into(),
        ));
    }
    let mut timing = Vec::with_capacity(query_timing.len());
    for (index, phone) in query_timing.iter().enumerate() {
        if phone.index != index + 1
            || !phone.start.is_finite()
            || phone.start < 0.0
            || !phone.duration.is_finite()
            || phone.duration < 0.0
        {
            return Err(Error("Invalid controlled query phone timing".into()));
        }
        let reference = encoded.references[phone.index];
        if reference[0] == 0 || reference[0] + 1 >= analysis.tokens.len() {
            return Err(Error(
                "Query phone timing contains a synthetic boundary token".into(),
            ));
        }
        timing.push(PhoneTiming {
            reference,
            symbol: encoded.symbols[phone.index].clone(),
            start: phone.start,
            duration: phone.duration,
        });
    }
    Ok(PhonePreview {
        segments,
        refs: encoded.references.clone(),
        nucleus: encoded.nucleus.clone(),
        codes,
        samples: total,
        timing,
    })
}
pub(super) fn preview(
    query: &PreparedQuery,
    controls: &Controls,
    analysis: &Analysis,
    style_offset: f32,
    edits: &Edits,
    invert: bool,
    sentence: &SentenceRef,
) -> CoreResult<PhonePreview> {
    phone_preview(
        query.encoded(),
        query.prosody(),
        &controls.values,
        query.raw_query_durations(),
        &query.intonation_coordinates(invert),
        &controls.phone_timing,
        analysis,
        style_offset,
        edits,
        invert,
        sentence,
    )
}
pub(super) fn unpack(
    mut render: Render,
    analysis: &Analysis,
    style_offset: f32,
    edits: &Edits,
    invert: bool,
    sentence: &SentenceRef,
) -> CoreResult<Baseline> {
    let pcm = render
        .stages
        .remove("pcm")
        .ok_or_else(|| Error("Missing PCM stage".into()))?;
    let controls = render
        .stages
        .get("controls")
        .ok_or_else(|| Error("Missing control stage".into()))?;
    let prosody = render
        .stages
        .get("prosody")
        .ok_or_else(|| Error("Missing prosody stage".into()))?;
    if pcm.cols() != 1 || pcm.rows() > MAX_AUDIO {
        return Err(Error("Rendered block audio limit or channel count".into()));
    }
    let phones = phone_preview(
        &render.encoded,
        prosody,
        controls,
        &render.raw_query_durations,
        &render.raw_query_intonation,
        &render.phone_timing,
        analysis,
        style_offset,
        edits,
        invert,
        sentence,
    )?;
    if phones.samples != pcm.rows() {
        return Err(Error("PCM length disagrees with phone/frame timing".into()));
    }
    Ok(Baseline {
        unit: UnitKey {
            value_policy: edits.value_policy,
            text: String::new(),
            parameters: Parameters::default(),
            project_parameters: Parameters::default(),
            narrator_parameters: Parameters::default(),
            scalar_layers: Default::default(),
        },
        dictionary: Arc::new(Vec::new()),
        voice: None,
        model_stamp: Vec::new(),
        dictionary_stamp: Vec::new(),
        weights: Vec::new(),
        pcm: Arc::new(pcm.values().to_vec()),
        segments: phones.segments,
        refs: phones.refs,
        nucleus: phones.nucleus,
        codes: phones.codes,
        timing: phones.timing,
        neutral_words: Vec::new(),
        word_query: Default::default(),
        word_positions: Default::default(),
    })
}
pub(super) fn annotate_preview(
    base: &PhonePreview,
    segments: &[Segment],
    edits: &mut Edits,
) -> CoreResult<bool> {
    annotate_parts(
        &base.segments,
        &base.refs,
        &base.nucleus,
        &base.codes,
        segments,
        edits,
    )
}
fn annotate_parts(
    base_segments: &[Segment],
    refs: &[[usize; 5]],
    nucleus: &[bool],
    base_codes: &[u32],
    segments: &[Segment],
    edits: &mut Edits,
) -> CoreResult<bool> {
    if segments.is_empty() {
        return Ok(false);
    }
    if segments.len() != base_segments.len()
        || segments.iter().zip(base_segments).any(|(a, b)| {
            a.sentence != b.sentence
                || a.symbol != b.symbol
                || a.reading != b.reading
                || a.token != b.token
                || a.mora != b.mora
                || a.phone != b.phone
                || a.token_text != b.token_text
        })
    {
        return Err(Error(
            "Canonical phone projection does not match its prepared query".into(),
        ));
    }
    let mut accents = BTreeMap::new();
    let mut pitch = BTreeMap::new();
    let mut changed = false;
    for (i, edit) in segments.iter().enumerate() {
        let slot = refs[i][3];
        if edit.accent_edited {
            if accents
                .insert(slot, edit.accent_code)
                .is_some_and(|v| v != edit.accent_code)
            {
                return Err(Error("Conflicting accent edits in one mora".into()));
            }
        }
        if edit.intonation_delta != 0.0 {
            if pitch
                .insert(slot, edit.intonation_delta)
                .is_some_and(|v| v != edit.intonation_delta)
            {
                return Err(Error("Conflicting intonation edits in one mora".into()));
            }
        }
        if edit.duration_edit.edited() {
            let value = match edit.duration_edit.mode {
                DurationMode::Relative => DurationEdit::Relative(edit.duration_edit.value),
                DurationMode::Absolute => DurationEdit::Absolute(edit.duration_edit.value),
            };
            edits.authored_duration.insert(i, value);
            changed = true;
        }
    }
    if !accents.is_empty() {
        let mut codes = base_codes.to_vec();
        for (i, reference) in refs.iter().enumerate() {
            if nucleus[i] {
                if let Some(code) = accents.get(&reference[3]) {
                    codes[i] = *code;
                }
            }
        }
        edits.accent_codes = Some(codes);
        changed = true;
    }
    if !pitch.is_empty() {
        edits.intonation_delta.extend(pitch);
        changed = true;
    }
    Ok(changed)
}
