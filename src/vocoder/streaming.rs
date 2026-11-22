mod buffers;
mod trace;
use super::{
    Render, Vocoder, VocoderModel, dsp, effects::SpectrumKind, lf_excitation, lf_math::Mode,
    lf_window, postfilter_log, response_exp, shape_transfer,
};
use crate::{
    Result,
    graph::{
        Budget,
        streaming::{Rows, Stream as Network},
    },
    invalid, pqmf,
    sequence::concatenate,
    tensor::{MAX_ELEMENTS, Tensor},
};
use trace::Trace;

const PACKET_FRAMES: usize = 16;

struct Pipeline<'a> {
    model: &'a VocoderModel,
    fused: bool,
    shape: Network<'a>,
    trunk: Network<'a>,
    harmonic: Network<'a>,
    noise_parameters: Network<'a>,
    postfilter: Network<'a>,
    projection: Network<'a>,
    lf: lf_excitation::Stream,
    harmonic_filter: dsp::LtvStream<'a>,
    noise_filter: dsp::LtvStream<'a>,
    analysis: pqmf::Analysis,
    synthesis: pqmf::Synthesis,
    upsample: dsp::Upsampler,
    noise: dsp::Noise,
    physical_rows: Rows,
    shape_rows: Rows,
    harmonic_parameters: Rows,
    harmonic_source: Rows,
    noise_parameter_rows: Rows,
    noise_source: Rows,
    random_rows: Rows,
    band_rows: Rows,
    harmonic_pcm: Rows,
    noise_pcm: Rows,
    trace: Trace,
    produced: usize,
    finished: bool,
}

impl<'a> Pipeline<'a> {
    fn new(vocoder: &'a Vocoder, tension: f32, intermediates: bool) -> Result<Self> {
        let fused = postfilter_log::fused_available();
        let model = vocoder.model.as_ref();
        let effect = |kind| {
            vocoder.effects.active(tension).then_some((
                &vocoder.effects,
                kind,
                tension,
                model.sample_rate,
            ))
        };
        Ok(Self {
            model,
            fused,
            shape: model.shape.stream()?,
            trunk: model.trunk.stream()?,
            harmonic: model.harmonic.stream()?,
            noise_parameters: model.noise.stream()?,
            postfilter: model.postfilter.stream()?,
            projection: model.projection.stream()?,
            lf: lf_excitation::Stream::new(
                lf_window::gain(model.excitation_log_gain, fused),
                model.input_configuration.force_voiced,
                Mode::current(),
            )?,
            harmonic_filter: dsp::LtvStream::new(model.spectral, effect(SpectrumKind::Periodic)),
            noise_filter: dsp::LtvStream::new(model.spectral, effect(SpectrumKind::Aperiodic)),
            analysis: pqmf::Analysis::new(&model.analysis, true)?,
            synthesis: pqmf::Synthesis::new(&model.synthesis)?,
            upsample: dsp::Upsampler::new(),
            noise: dsp::Noise::new(fused),
            physical_rows: Rows::default(),
            shape_rows: Rows::default(),
            harmonic_parameters: Rows::default(),
            harmonic_source: Rows::default(),
            noise_parameter_rows: Rows::default(),
            noise_source: Rows::default(),
            random_rows: Rows::default(),
            band_rows: Rows::default(),
            harmonic_pcm: Rows::default(),
            noise_pcm: Rows::default(),
            trace: Trace::new(intermediates),
            produced: 0,
            finished: false,
        })
    }

    fn feed(
        &mut self,
        physical: &Tensor,
        end: bool,
        budget: &mut Budget,
        emit: &mut dyn FnMut(&[f32]) -> Result<()>,
    ) -> Result<()> {
        let mut logged = physical.clone();
        budget.spend(0, logged.values().len())?;
        for row in logged.values_mut().chunks_exact_mut(72) {
            if self.model.input_configuration.force_voiced {
                row[0] = 1.0;
            }
            if row[1] <= 0.0 {
                return Err(invalid("vocoder F0 must be positive"));
            }
            row[1] = postfilter_log::logarithm(row[1], self.fused);
        }
        let normalized = self.model.normalizer.normalize(&logged, true)?;
        let normalized = self
            .model
            .input_configuration
            .expand(normalized, self.fused, budget)?;
        self.trace.push("normalized", Some(&normalized), budget)?;
        let shape = self
            .shape
            .feed(Some(&normalized), None, end, false, budget)?;
        self.shape_rows
            .push(shape.data.as_ref(), shape.end, budget)?;
        self.physical_rows.push(Some(physical), end, budget)?;
        let (physical, logits, source_end) =
            buffers::pair(&mut self.physical_rows, &mut self.shape_rows, 1, budget)?;
        let source = if let (Some(physical), Some(logits)) = (physical, logits) {
            if logits.cols() != 1 {
                return Err(invalid("vocoder shape output width"));
            }
            let shape = Tensor::new(
                logits.rows(),
                1,
                logits
                    .values()
                    .iter()
                    .map(|value| shape_transfer::transfer(*value, 0.0, self.fused))
                    .collect(),
            )?;
            self.trace.push("shape", Some(&shape), budget)?;
            Some(self.lf.feed(&physical, &shape, source_end, budget)?)
        } else if source_end {
            Some(
                self.lf
                    .feed(&Tensor::zeros(0, 72)?, &Tensor::zeros(0, 1)?, true, budget)?,
            )
        } else {
            None
        };
        self.trace.push("source", source.as_ref(), budget)?;
        self.harmonic_source
            .push(source.as_ref(), source_end, budget)?;
        let hidden = self
            .trunk
            .feed(Some(&normalized), None, end, false, budget)?;
        self.trace.push("hidden", hidden.data.as_ref(), budget)?;
        let harmonic_parameters =
            self.harmonic
                .feed(hidden.data.as_ref(), None, hidden.end, false, budget)?;
        self.trace.push(
            "harmonic-parameters",
            harmonic_parameters.data.as_ref(),
            budget,
        )?;
        self.harmonic_parameters.push(
            harmonic_parameters.data.as_ref(),
            harmonic_parameters.end,
            budget,
        )?;
        let noise_parameters =
            self.noise_parameters
                .feed(hidden.data.as_ref(), None, hidden.end, false, budget)?;
        self.trace
            .push("noise-parameters", noise_parameters.data.as_ref(), budget)?;
        self.noise_parameter_rows.push(
            noise_parameters.data.as_ref(),
            noise_parameters.end,
            budget,
        )?;
        let harmonic = buffers::ltv(
            &mut self.harmonic_filter,
            &mut self.harmonic_parameters,
            &mut self.harmonic_source,
            budget,
        )?;
        self.trace
            .push("harmonic", harmonic.data.as_ref(), budget)?;
        self.harmonic_pcm
            .push(harmonic.data.as_ref(), harmonic.end, budget)?;

        let (post_source, post_end) = if self.model.filtered_postfilter_input {
            (harmonic.data.as_ref(), harmonic.end)
        } else {
            (source.as_ref(), source_end)
        };
        let subbands = match post_source {
            Some(source) => self.analysis.feed(source, post_end, budget)?,
            None => self
                .analysis
                .feed(&Tensor::zeros(0, 1)?, post_end, budget)?,
        };
        self.trace.push("subbands", Some(&subbands), budget)?;
        let noise = self.noise.generate(subbands.rows(), budget)?;
        self.trace.push("noise", Some(&noise), budget)?;
        self.random_rows.push(Some(&noise), post_end, budget)?;
        let input = concatenate(&[&subbands, &noise], budget)?;
        let upsampled = self.upsample.feed(&normalized, end, budget)?;
        self.trace.push("upsampled", Some(&upsampled), budget)?;
        let bands = self
            .postfilter
            .feed(Some(&input), Some(&upsampled), post_end, end, budget)?;
        let bands = self
            .projection
            .feed(bands.data.as_ref(), None, bands.end, false, budget)?;
        self.trace.push("bands", bands.data.as_ref(), budget)?;
        self.band_rows
            .push(bands.data.as_ref(), bands.end, budget)?;
        let (bands, noise, gate_end) =
            buffers::pair(&mut self.band_rows, &mut self.random_rows, 1, budget)?;
        let gated = if let (Some(bands), Some(noise)) = (bands, noise) {
            if bands.cols() != noise.cols() {
                return Err(invalid("vocoder postfilter dimensions"));
            }
            let gated = if self.model.gate_noise {
                Tensor::new(
                    bands.rows(),
                    bands.cols(),
                    bands
                        .values()
                        .iter()
                        .zip(noise.values())
                        .map(|(band, noise)| {
                            (1.0 / (1.0 + response_exp::exponential(-*band, self.fused))) * noise
                        })
                        .collect(),
                )?
            } else {
                bands
            };
            self.trace.push("gated", Some(&gated), budget)?;
            gated
        } else {
            Tensor::zeros(0, 8)?
        };
        let aperiodic_source = self.synthesis.feed(&gated, gate_end, budget)?;
        self.trace
            .push("aperiodic-source", Some(&aperiodic_source), budget)?;
        self.noise_source
            .push(Some(&aperiodic_source), gate_end, budget)?;
        let aperiodic = buffers::ltv(
            &mut self.noise_filter,
            &mut self.noise_parameter_rows,
            &mut self.noise_source,
            budget,
        )?;
        self.trace
            .push("aperiodic", aperiodic.data.as_ref(), budget)?;
        self.noise_pcm
            .push(aperiodic.data.as_ref(), aperiodic.end, budget)?;
        let (harmonic, aperiodic, complete) =
            buffers::pair(&mut self.harmonic_pcm, &mut self.noise_pcm, 1, budget)?;
        if let (Some(harmonic), Some(aperiodic)) = (harmonic, aperiodic) {
            let pcm = Tensor::new(
                harmonic.rows(),
                1,
                harmonic
                    .values()
                    .iter()
                    .zip(aperiodic.values())
                    .map(|(harmonic, aperiodic)| harmonic + aperiodic)
                    .collect(),
            )?;
            self.trace.push("pcm", Some(&pcm), budget)?;
            emit(pcm.values())?;
            self.produced += pcm.rows();
        }
        self.finished = complete;
        Ok(())
    }
}

pub(crate) struct PcmPackets<'a> {
    pipeline: Pipeline<'a>,
    input: Rows,
    expected_frames: usize,
    received: usize,
}

impl<'a> PcmPackets<'a> {
    pub(crate) fn new(
        model: &'a Vocoder,
        frames: usize,
        tension: f32,
        budget: &mut Budget,
    ) -> Result<Self> {
        if !tension.is_finite() || frames == 0 || frames > MAX_ELEMENTS / dsp::HOP {
            return Err(invalid("vocoder stream length or tension"));
        }
        budget.spend(0, frames * dsp::HOP)?;
        Ok(Self {
            pipeline: Pipeline::new(model, tension, false)?,
            input: Rows::default(),
            expected_frames: frames,
            received: 0,
        })
    }

    pub(crate) fn feed(
        &mut self,
        physical: &Tensor,
        end: bool,
        budget: &mut Budget,
        emit: &mut dyn FnMut(&[f32]) -> Result<()>,
    ) -> Result<()> {
        if physical.cols() != 72 || self.input.input_ended() {
            return Err(invalid("vocoder packet dimensions or end"));
        }
        self.received += physical.rows();
        if self.received > self.expected_frames || (end && self.received != self.expected_frames) {
            return Err(invalid("vocoder packet frame count"));
        }
        self.input.push(Some(physical), end, budget)?;
        while self.input.rows() > PACKET_FRAMES || (end && self.input.rows() != 0) {
            let allocation_limit = budget.output_elements;
            let packet = self
                .input
                .take(self.input.rows().min(PACKET_FRAMES), budget)?
                .ok_or_else(|| invalid("missing vocoder packet"))?;
            self.pipeline
                .feed(&packet, self.input.end(), budget, emit)?;
            budget.output_elements = allocation_limit;
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<Tensor> {
        if !self.pipeline.finished || self.pipeline.produced != self.expected_frames * dsp::HOP {
            return Err(invalid(
                "vocoder stream did not finish at its prepared length",
            ));
        }
        self.pipeline
            .trace
            .finish()?
            .remove("pcm")
            .ok_or_else(|| invalid("vocoder did not return PCM"))
    }
}

pub(super) fn render(
    model: &Vocoder,
    physical: &Tensor,
    tension: f32,
    budget: &mut Budget,
    emit: &mut dyn FnMut(&[f32]) -> Result<()>,
    intermediates: bool,
) -> Result<Render> {
    if !tension.is_finite()
        || physical.cols() != 72
        || physical.rows() == 0
        || physical.rows() > MAX_ELEMENTS / dsp::HOP
    {
        return Err(invalid(
            "vocoder requires nonempty bounded 72-channel frames",
        ));
    }
    let mut pipeline = Pipeline::new(model, tension, intermediates)?;
    let packet_allocation_limit = if intermediates {
        None
    } else {
        // Retain one PCM result and reuse the allocation allowance for packet scratch.
        budget.spend(0, physical.rows() * dsp::HOP)?;
        Some(budget.output_elements)
    };
    for first in (0..physical.rows()).step_by(PACKET_FRAMES) {
        let end = (first + PACKET_FRAMES).min(physical.rows());
        budget.spend(0, (end - first) * physical.cols())?;
        let packet = Tensor::new(
            end - first,
            physical.cols(),
            physical.values()[first * physical.cols()..end * physical.cols()].to_vec(),
        )?;
        pipeline.feed(&packet, end == physical.rows(), budget, emit)?;
        if let Some(limit) = packet_allocation_limit {
            budget.output_elements = limit;
        }
    }
    if !pipeline.finished || pipeline.produced != physical.rows() * dsp::HOP {
        return Err(invalid(
            "vocoder stream did not finish at its prepared length",
        ));
    }
    Ok(Render {
        stages: pipeline.trace.finish()?,
    })
}
