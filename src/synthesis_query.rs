//! Immutable per-sentence query preparation. Native text-change jobs prepare queries
//! before Play; acoustic sampling and PCM creation remain a separate operation.
use super::{Edits, Parameters, Render, RenderTimings, Synthesizer};
use crate::{
    Result,
    graph::Budget,
    invalid,
    phonemes::{Encoded, Token},
    stylebook::Style,
    tensor::Tensor,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// Owned intermediate query tied to the compiled models that produced it.
/// Scalar, duration, pitch and seed changes can reuse it; prediction/accent changes cannot.
pub struct PreparedQuery {
    pub(super) identity: Arc<()>,
    pub(super) encoded: Encoded,
    pub(super) language: Tensor,
    pub(super) prosody: Tensor,
    pub(super) style: Style,
    pub(super) raw_query_durations: Vec<f32>,
    pub(super) prediction_mask: Option<Vec<bool>>,
    source_prediction: Option<Vec<Vec<bool>>>,
    source_accent_codes: Option<Vec<u32>>,
    pub(super) timings: Option<RenderTimings>,
}
impl PreparedQuery {
    pub fn encoded(&self) -> &Encoded {
        &self.encoded
    }
    pub fn language(&self) -> &Tensor {
        &self.language
    }
    pub fn prosody(&self) -> &Tensor {
        &self.prosody
    }
    pub fn raw_query_durations(&self) -> &[f32] {
        &self.raw_query_durations
    }
    pub fn intonation_coordinates(&self, invert: bool) -> Vec<f32> {
        super::intonation::coordinates(&self.encoded, &self.prosody, &self.style, invert)
    }
    pub fn prediction_mask(&self) -> Option<&[bool]> {
        self.prediction_mask.as_deref()
    }
    /// Preparation timings contain no controls, acoustic or vocoder work.
    pub fn timings(&self) -> Option<&RenderTimings> {
        self.timings.as_ref()
    }

    /// Retained query charge including inline storage; model owners use heap-only charges.
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        m::sum([
            std::mem::size_of::<Self>(),
            m::arc::<()>(0),
            m::values(&self.raw_query_durations),
            self.encoded.estimated_bytes(),
            self.language.estimated_bytes(),
            self.prosody.estimated_bytes(),
            self.style.estimated_bytes(),
            m::optional(&self.prediction_mask, m::values),
            m::optional(&self.source_accent_codes, m::values),
            m::optional(&self.source_prediction, |rows| m::nested(rows, m::values)),
        ])
    }
    pub(super) fn validate(&self, synth: &Synthesizer, edits: &Edits) -> Result<()> {
        if !Arc::ptr_eq(&self.identity, &synth.identity) {
            return Err(invalid(
                "prepared query belongs to different compiled models",
            ));
        }
        if self.source_prediction != edits.predict || self.source_accent_codes != edits.accent_codes
        {
            return Err(invalid("prepared query prediction/accent inputs changed"));
        }
        Ok(())
    }
    pub(super) fn copy_encoded(&self) -> Encoded {
        Encoded {
            linguistic: self.encoded.linguistic.clone(),
            references: self.encoded.references.clone(),
            nucleus: self.encoded.nucleus.clone(),
            codes: self.encoded.codes.clone(),
            symbols: self.encoded.symbols.clone(),
        }
    }
}
impl Synthesizer {
    /// Prepare encoding, language, style and prosody without sampling acoustic noise or PCM.
    pub fn prepare_edited(
        &self,
        tokens: &[Token],
        weights: &[f32],
        edits: &Edits,
        budget: &mut Budget,
    ) -> Result<PreparedQuery> {
        self.prepare_query_impl(tokens, weights, edits, budget, false)
    }
    pub fn prepare_edited_profiled(
        &self,
        tokens: &[Token],
        weights: &[f32],
        edits: &Edits,
        budget: &mut Budget,
    ) -> Result<PreparedQuery> {
        self.prepare_query_impl(tokens, weights, edits, budget, true)
    }
    /// Apply current scalar/curve/seed edits and generate audio from an immutable query.
    pub fn render_prepared(
        &self,
        prepared: &PreparedQuery,
        params: &Parameters,
        edits: &Edits,
        budget: &mut Budget,
    ) -> Result<Render> {
        let scalars = self.validate_parameters(params, edits)?;
        self.render_prepared_impl(prepared, params, edits, budget, false, scalars)
    }
    /// Timings here describe newly performed controls/acoustic/vocoder work only.
    pub fn render_prepared_profiled(
        &self,
        prepared: &PreparedQuery,
        params: &Parameters,
        edits: &Edits,
        budget: &mut Budget,
    ) -> Result<Render> {
        let scalars = self.validate_parameters(params, edits)?;
        self.render_prepared_impl(prepared, params, edits, budget, true, scalars)
    }
    /// Apply current UI scalar/curve edits to query durations/intonation without audio inference.
    pub fn controls_prepared(
        &self,
        prepared: &PreparedQuery,
        params: &Parameters,
        edits: &Edits,
    ) -> Result<super::Controls> {
        self.validate_parameters(params, edits)?;
        prepared.validate(self, edits)?;
        self.prepared_controls_impl(prepared, params, edits)
    }
    pub(super) fn prepared_controls_impl(
        &self,
        prepared: &PreparedQuery,
        params: &Parameters,
        edits: &Edits,
    ) -> Result<super::Controls> {
        let period = match &self.acoustic {
            super::Acoustic::Legacy(model) => model.frame_period,
            super::Acoustic::Modern(_, model, _, _) => model.frame_period,
        };
        super::prepare_controls(
            &prepared.encoded,
            &prepared.prosody,
            &prepared.style,
            &prepared.raw_query_durations,
            period,
            params,
            edits,
        )
    }
    pub(super) fn validate_tokens(tokens: &[Token]) -> Result<()> {
        if tokens.len() < 2 || tokens[0].pos != 0x2000 || tokens.last().unwrap().pos != 0x2001 {
            return Err(invalid("render requires explicit BOS and EOS tokens"));
        }
        Ok(())
    }
    pub(super) fn prepare_query_impl(
        &self,
        tokens: &[Token],
        weights: &[f32],
        edits: &Edits,
        budget: &mut Budget,
        collect_timings: bool,
    ) -> Result<PreparedQuery> {
        Self::validate_tokens(tokens)?;
        let started = collect_timings.then(Instant::now);
        let mut checkpoint = started;
        let mut stage_elapsed = || {
            checkpoint.as_mut().map_or(Duration::ZERO, |previous| {
                let now = Instant::now();
                let elapsed = now.duration_since(*previous);
                *previous = now;
                elapsed
            })
        };
        let x = self.encoder.encode(tokens)?;
        if x.linguistic.rows() < 2 {
            return Err(invalid("render needs boundary phonemes"));
        }
        let encoding_elapsed = stage_elapsed();
        let lm = self.lm.run_tokens(tokens, budget)?;
        let language_elapsed = stage_elapsed();
        let words: Vec<_> = x.references.iter().map(|r| r[0]).collect();
        let predict = edits
            .predict
            .as_ref()
            .map(|mask| {
                if mask.len() != tokens.len()
                    || mask
                        .iter()
                        .zip(tokens)
                        .any(|(row, token)| row.len() != token.syllables.len())
                {
                    return Err(invalid("token/syllable prediction mask dimensions"));
                }
                Ok(x.references
                    .iter()
                    .map(|r| mask[r[0]][r[1]])
                    .collect::<Vec<_>>())
            })
            .transpose()?;
        let style = self.stylebook.mix(weights)?;
        let predicted = self.prosody.run_with_prediction(
            &x.linguistic,
            &lm,
            &words,
            predict.as_deref(),
            edits.accent_codes.as_deref(),
            &self.speaker_im,
            &self.speaker_accent,
            &style.embedding,
            budget,
        )?;
        let period = match &self.acoustic {
            super::Acoustic::Legacy(model) => model.frame_period,
            super::Acoustic::Modern(_, model, _, _) => model.frame_period,
        };
        let raw_query_durations = super::duration::raw(&x, &predicted, &style, period)?;
        let prosody_elapsed = stage_elapsed();
        let timings = started.map(|started| RenderTimings {
            encoding: encoding_elapsed,
            language: language_elapsed,
            prosody: prosody_elapsed,
            total: started.elapsed(),
            ..Default::default()
        });
        Ok(PreparedQuery {
            identity: Arc::clone(&self.identity),
            encoded: x,
            language: lm,
            prosody: predicted,
            style,
            raw_query_durations,
            prediction_mask: predict,
            source_prediction: edits.predict.clone(),
            source_accent_codes: edits.accent_codes.clone(),
            timings,
        })
    }
}
