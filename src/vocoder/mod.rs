//! LF excitation, learned time-varying filters and subband neural noise.
//! Float DSP behavior is independently qualified against reference arrays.
mod analytic_probe;
mod complex_fft;
mod dsp;
mod effects;
mod exp_scale;
mod harmonic_probe;
mod input_probe;
mod lf_atan;
mod lf_coefficients;
mod lf_czt;
mod lf_excitation;
mod lf_harmonics;
mod lf_math;
mod lf_spectrum;
mod lf_table;
mod lf_trig;
mod lf_window;
mod ltv_window;
mod postfilter_log;
mod real_fft;
mod response;
mod response_exp;
mod response_trig;
mod shape_transfer;
mod source_probe;
mod transform_probe;
use crate::{
    Result,
    dnni::Node,
    graph::{Budget, Graph},
    invalid,
    normalization::Normalizer,
    sequence::concatenate,
    tensor::{MAX_ELEMENTS, Tensor, u32_at},
};
pub use analytic_probe::{
    LfArguments, LfSpectrum, diagnostic_lf_arguments, diagnostic_lf_spectrum,
};
pub use effects::{Effects, SpectrumKind};
pub use harmonic_probe::{LfHarmonics, diagnostic_lf_harmonics, diagnostic_lf_phase_step};
pub use input_probe::{VocoderLogarithms, diagnostic_vocoder_logarithms};
pub use source_probe::{LfSource, diagnostic_lf_source};
use std::collections::BTreeMap;
pub use transform_probe::{
    LfCzt, diagnostic_lf_chirp_arguments, diagnostic_lf_complex_fft, diagnostic_lf_czt,
    diagnostic_lf_generated_czt,
};

pub fn response_exp(log_amplitude: f32, scale: f32) -> Result<f32> {
    if !log_amplitude.is_finite() || !scale.is_finite() {
        return Err(invalid("response exponential requires finite inputs"));
    }
    Ok(exp_scale::scaled_exp(log_amplitude, scale))
}

pub struct CepstralResponseBoundary {
    pub packed_input: Vec<f32>,
    pub packed_transform: Vec<f32>,
}

pub fn diagnostic_cepstral_response(parameters: &[f32]) -> Result<CepstralResponseBoundary> {
    let mut buffer = response::cepstral_input(parameters)?;
    let packed_input = buffer.to_vec();
    real_fft::forward(&mut buffer);
    if buffer.iter().any(|value| !value.is_finite()) {
        return Err(invalid(
            "cepstral response probe produced non-finite values",
        ));
    }
    Ok(CepstralResponseBoundary {
        packed_input,
        packed_transform: buffer.to_vec(),
    })
}

pub fn diagnostic_response_endpoint(
    input: &[f32],
    fused: Option<bool>,
) -> Result<(Vec<f32>, bool)> {
    if input.is_empty() || input.len() > 8192 || input.iter().any(|value| !value.is_finite()) {
        return Err(invalid(
            "response endpoint probe requires 1..8192 finite F32 words",
        ));
    }
    let fused = fused.unwrap_or_else(postfilter_log::fused_available);
    let output: Vec<f32> = input
        .iter()
        .map(|value| response_exp::exponential(*value, fused))
        .collect();
    if output.iter().any(|value| !value.is_finite()) {
        return Err(invalid(
            "response endpoint probe produced non-finite values",
        ));
    }
    Ok((output, fused))
}

pub struct ResponsePhase {
    pub fused: bool,
    pub cosine: Vec<f32>,
    pub sine: Vec<f32>,
}

pub fn diagnostic_response_phase(input: &[f32], fused: Option<bool>) -> Result<ResponsePhase> {
    if input.is_empty() || input.len() > 8192 || input.iter().any(|value| !value.is_finite()) {
        return Err(invalid(
            "response phase probe requires 1..8192 finite F32 words",
        ));
    }
    let fused = fused.unwrap_or_else(postfilter_log::fused_available);
    let cosine: Vec<f32> = input
        .iter()
        .map(|value| response_trig::cosine(*value, fused))
        .collect();
    let sine: Vec<f32> = input
        .iter()
        .map(|value| response_trig::sine(*value, fused))
        .collect();
    if cosine.iter().chain(&sine).any(|value| !value.is_finite()) {
        return Err(invalid("response phase probe produced non-finite values"));
    }
    Ok(ResponsePhase {
        fused,
        cosine,
        sine,
    })
}

pub struct LfShape {
    pub fused: bool,
    pub values: Vec<f32>,
}

pub struct LfCoefficients {
    pub fused: bool,
    pub raw: Vec<[f32; 5]>,
    pub values: Vec<[f32; 5]>,
}

pub fn diagnostic_lf_coefficients(shapes: &[f32], fused: Option<bool>) -> Result<LfCoefficients> {
    if shapes.is_empty() || shapes.len() > 8192 || shapes.iter().any(|value| !value.is_finite()) {
        return Err(invalid(
            "LF coefficient probe requires 1..8192 finite shapes",
        ));
    }
    let fused = fused.unwrap_or_else(postfilter_log::fused_available);
    let raw: Vec<[f32; 5]> = shapes
        .iter()
        .map(|shape| lf_coefficients::interpolate(*shape))
        .collect();
    let values: Vec<[f32; 5]> = raw
        .iter()
        .map(|value| lf_coefficients::convert(*value, fused))
        .collect();
    if values.iter().flatten().any(|value| !value.is_finite()) {
        return Err(invalid("LF coefficient probe produced non-finite values"));
    }
    Ok(LfCoefficients { fused, raw, values })
}

pub struct LfWindow {
    pub fused: bool,
    pub gain: f32,
    pub window: Vec<f32>,
    pub overlap: Vec<f32>,
}

pub fn diagnostic_lf_window(
    log_gain: f32,
    source: &[f32],
    existing: &[f32],
    fused: Option<bool>,
) -> Result<LfWindow> {
    if !log_gain.is_finite()
        || source.len() != lf_window::LENGTH
        || existing.len() != lf_window::LENGTH
        || source
            .iter()
            .chain(existing)
            .any(|value| !value.is_finite())
    {
        return Err(invalid(
            "LF window probe requires finite gain and 480/480 sample arrays",
        ));
    }
    let fused = fused.unwrap_or_else(postfilter_log::fused_available);
    let gain = lf_window::gain(log_gain, fused);
    let window = lf_window::window(gain, fused).to_vec();
    let overlap: Vec<f32> = source
        .iter()
        .zip(&window)
        .zip(existing)
        .map(|((source, window), existing)| lf_window::accumulate(*source, *window, *existing))
        .collect();
    if !gain.is_finite()
        || window
            .iter()
            .chain(&overlap)
            .any(|value| !value.is_finite())
    {
        return Err(invalid("LF window probe produced non-finite values"));
    }
    Ok(LfWindow {
        fused,
        gain,
        window,
        overlap,
    })
}

pub fn diagnostic_lf_shape(
    logits: &[f32],
    tensions: &[f32],
    fused: Option<bool>,
) -> Result<LfShape> {
    if logits.is_empty()
        || logits.len() > 8192
        || logits.len() != tensions.len()
        || logits
            .iter()
            .chain(tensions)
            .any(|value| !value.is_finite())
    {
        return Err(invalid(
            "LF shape probe requires 1..8192 paired finite values",
        ));
    }
    let fused = fused.unwrap_or_else(postfilter_log::fused_available);
    let values: Vec<f32> = logits
        .iter()
        .zip(tensions)
        .map(|(logit, tension)| shape_transfer::transfer(*logit, *tension, fused))
        .collect();
    if values.iter().any(|value| !value.is_finite()) {
        return Err(invalid("LF shape probe produced non-finite values"));
    }
    Ok(LfShape { fused, values })
}

pub fn postfilter_noise(rows: usize, budget: &mut Budget) -> Result<Tensor> {
    dsp::postfilter_noise(rows, budget)
}

pub fn diagnostic_postfilter_noise(
    rows: usize,
    fused: Option<bool>,
    budget: &mut Budget,
) -> Result<(Tensor, bool)> {
    let fused = fused.unwrap_or_else(postfilter_log::fused_available);
    Ok((dsp::postfilter_noise_mode(rows, fused, budget)?, fused))
}

pub struct LtvBoundaries {
    pub fused: bool,
    pub window: Vec<f32>,
    pub windowed: Vec<f32>,
    pub overlap: Vec<f32>,
}

fn fft_input(input: &[f32]) -> Result<[f32; 1024]> {
    if input.len() != 1024 || input.iter().any(|value| !value.is_finite()) {
        return Err(invalid(
            "real FFT probe requires exactly 1024 finite F32 words",
        ));
    }
    input
        .try_into()
        .map_err(|_| invalid("real FFT input dimensions"))
}

pub fn diagnostic_real_fft(input: &[f32], inverse: bool) -> Result<Vec<f32>> {
    let mut output = fft_input(input)?;
    if inverse {
        real_fft::inverse(&mut output);
    } else {
        real_fft::forward(&mut output);
    }
    if output.iter().any(|value| !value.is_finite()) {
        return Err(invalid("real FFT probe produced non-finite words"));
    }
    Ok(output.to_vec())
}

pub struct LtvFft {
    pub spectrum: Vec<f32>,
    pub product: Vec<f32>,
    pub inverse: Vec<f32>,
}

/// Source is already windowed and padded; response uses 1024 packed real-FFT words.
pub fn diagnostic_ltv_fft(source: &[f32], response: &[f32]) -> Result<LtvFft> {
    let mut buffer = fft_input(source)?;
    let response = fft_input(response)?;
    real_fft::forward(&mut buffer);
    let spectrum = buffer.to_vec();
    real_fft::multiply(&mut buffer, &response);
    let product = buffer.to_vec();
    real_fft::inverse(&mut buffer);
    if spectrum
        .iter()
        .chain(&product)
        .chain(&buffer)
        .any(|value| !value.is_finite())
    {
        return Err(invalid("LTV FFT probe produced non-finite words"));
    }
    Ok(LtvFft {
        spectrum,
        product,
        inverse: buffer.to_vec(),
    })
}

pub fn diagnostic_ltv_boundaries(
    source: &[f32],
    unscaled: &[f32],
    existing_overlap: &[f32],
    fused: Option<bool>,
) -> Result<LtvBoundaries> {
    if source.len() != 480
        || unscaled.len() != 1024
        || existing_overlap.len() != 1024
        || source
            .iter()
            .chain(unscaled)
            .chain(existing_overlap)
            .any(|value| !value.is_finite())
    {
        return Err(invalid(
            "LTV boundary probe requires finite 480/1024/1024 sample arrays",
        ));
    }
    let fused = fused.unwrap_or_else(postfilter_log::fused_available);
    let window = ltv_window::window_with_mode(fused).to_vec();
    let windowed = source
        .iter()
        .zip(&window)
        .map(|(source, window)| ltv_window::source_product(*source, *window))
        .collect();
    let mut overlap = existing_overlap.to_vec();
    ltv_window::overlap(&mut overlap, unscaled);
    if overlap.iter().any(|value| !value.is_finite()) {
        return Err(invalid("LTV boundary probe produced non-finite samples"));
    }
    Ok(LtvBoundaries {
        fused,
        window,
        windowed,
        overlap,
    })
}

pub struct Vocoder {
    normalizer: Normalizer,
    shape: Graph,
    trunk: Graph,
    harmonic: Graph,
    noise: Graph,
    analysis: Tensor,
    synthesis: Tensor,
    postfilter: Graph,
    projection: Graph,
    excitation_log_gain: f32,
    spectral: bool,
    gate_noise: bool,
    filtered_postfilter_input: bool,
    effects: Effects,
    sample_rate: f32,
}
pub struct Render {
    pub stages: BTreeMap<&'static str, Tensor>,
}
/// Boundary probe with an explicit shape vector, separating DSP from shape-network error.
pub fn lf_source(
    physical: &Tensor,
    shape: &Tensor,
    gain: f32,
    budget: &mut Budget,
) -> Result<Tensor> {
    if physical.rows() > MAX_ELEMENTS / dsp::HOP {
        return Err(invalid("excitation sample limit"));
    }
    lf_excitation::excitation(physical, shape, gain, budget)
}
fn taps(n: &Node<'_>, tag: &str) -> Result<Tensor> {
    if n.tag != tag || n.children.len() != 8 || !n.payload.is_empty() {
        return Err(invalid("vocoder PQMF structure"));
    }
    let mut values = Vec::with_capacity(8 * 65);
    for c in &n.children {
        let v = c.tensor()?;
        if v.values().len() != 65 {
            return Err(invalid("vocoder PQMF taps"));
        }
        values.extend_from_slice(v.values());
    }
    Tensor::new(8, 65, values)
}
impl Vocoder {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.normalizer.estimated_bytes(),
            self.shape.estimated_bytes(),
            self.trunk.estimated_bytes(),
            self.harmonic.estimated_bytes(),
            self.noise.estimated_bytes(),
            self.analysis.estimated_bytes(),
            self.synthesis.estimated_bytes(),
            self.postfilter.estimated_bytes(),
            self.projection.estimated_bytes(),
            self.effects.estimated_bytes(),
        ])
    }

    pub fn compile(root: &Node<'_>) -> Result<Self> {
        if !["_nhvpl2", "_nhvpl3"].contains(&root.tag.as_str())
            || root.children.len() != 9
            || root.payload.len() != if root.tag == "_nhvpl2" { 17 } else { 18 }
        {
            return Err(invalid("vocoder root"));
        }
        if u32_at(root.payload, 0)? != 8 || root.payload[12..].iter().any(|flag| *flag > 1) {
            return Err(invalid("vocoder configuration"));
        }
        if root.payload[12] != 1
            || root.payload[13] != 1
            || root.payload[14] != 1
            || root.payload[15] != 0
        {
            return Err(invalid(
                "unsupported vocoder shape/trunk/postfilter branch combination",
            ));
        }
        let excitation_log_gain = f32::from_bits(u32_at(root.payload, 4)?);
        let noise_log_gain = f32::from_bits(u32_at(root.payload, 8)?);
        if !excitation_log_gain.is_finite() || !noise_log_gain.is_finite() {
            return Err(invalid("vocoder gains"));
        }
        let c = &root.children;
        if c[0].children.is_empty()
            || c[3].children.len() != 1
            || c[4].children.len() != 1
            || !["_ltvfl0", "_ltvfl1"].contains(&c[3].tag.as_str())
            || c[4].tag != c[3].tag
        {
            return Err(invalid("vocoder filter configuration"));
        }
        if u32_at(c[0].payload, 8)? != 48000 || f32::from_bits(u32_at(c[0].payload, 12)?) != 0.005 {
            return Err(invalid("vocoder requires 48 kHz and 5 ms frame spacing"));
        }
        Ok(Self {
            normalizer: Normalizer::from_node(&c[0].children[0])?,
            shape: Graph::compile(&c[1])?,
            trunk: Graph::compile(&c[2])?,
            harmonic: Graph::compile(&c[3].children[0])?,
            noise: Graph::compile(&c[4].children[0])?,
            synthesis: taps(&c[5], "_ppusv0")?,
            analysis: taps(&c[6], "_ppdsv0")?,
            postfilter: Graph::compile(&c[7])?,
            projection: Graph::compile(&c[8])?,
            excitation_log_gain,
            spectral: c[3].tag == "_ltvfl1",
            gate_noise: root.payload[16] != 0,
            filtered_postfilter_input: root.tag == "_nhvpl2" || root.payload[17] != 0,
            effects: Effects::default(),
            sample_rate: u32_at(c[0].payload, 8)? as f32,
        })
    }
    pub fn set_effects(&mut self, effects: Effects) {
        self.effects = effects;
    }
    pub fn run(&self, physical: &Tensor, budget: &mut Budget) -> Result<Render> {
        self.run_with_tension(physical, 0.0, budget)
    }
    pub fn run_with_tension(
        &self,
        physical: &Tensor,
        tension: f32,
        budget: &mut Budget,
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
        let fused = postfilter_log::fused_available();
        let mut logged = physical.clone();
        for t in 0..physical.rows() {
            if logged.row(t)[1] <= 0.0 {
                return Err(invalid("vocoder F0 must be positive"));
            }
            logged.row_mut(t)[1] = postfilter_log::logarithm(logged.row(t)[1], fused);
        }
        let normalized = self.normalizer.normalize(&logged, true)?;
        let logits = self.shape.run_with(&normalized, budget)?;
        if logits.cols() != 1 {
            return Err(invalid("vocoder shape output width"));
        }
        let shape = Tensor::new(
            logits.rows(),
            1,
            logits
                .values()
                .iter()
                .map(|x| shape_transfer::transfer(*x, 0.0, fused))
                .collect(),
        )?;
        let hidden = self.trunk.run_with(&normalized, budget)?;
        let source = lf_excitation::excitation(
            physical,
            &shape,
            lf_window::gain(self.excitation_log_gain, fused),
            budget,
        )?;
        let harmonic_parameters = self.harmonic.run_with(&hidden, budget)?;
        let effect = |kind| {
            self.effects
                .active(tension)
                .then_some((&self.effects, kind, tension, self.sample_rate))
        };
        let harmonic = dsp::ltv(
            &harmonic_parameters,
            &source,
            self.spectral,
            effect(SpectrumKind::Periodic),
            budget,
        )?;
        let postfilter_source = if self.filtered_postfilter_input {
            &harmonic
        } else {
            &source
        };
        let subbands = dsp::analysis(postfilter_source, &self.analysis, budget)?;
        // The active neural postfilter takes unit Gaussian noise. The root's
        // noise gain belongs to the separate branch without that postfilter.
        let noise = dsp::postfilter_noise(subbands.rows(), budget)?;
        let input = concatenate(&[&subbands, &noise], budget)?;
        let upsampled = dsp::upsample(&normalized, budget)?;
        let bands = self.projection.run_with(
            &self
                .postfilter
                .run_conditioned(&input, &upsampled, budget)?,
            budget,
        )?;
        if bands.rows() != noise.rows() || bands.cols() != noise.cols() {
            return Err(invalid("vocoder postfilter dimensions"));
        }
        let gated = if self.gate_noise {
            let fused = postfilter_log::fused_available();
            Tensor::new(
                bands.rows(),
                bands.cols(),
                bands
                    .values()
                    .iter()
                    .zip(noise.values())
                    .map(|(b, n)| (1.0 / (1.0 + response_exp::exponential(-*b, fused))) * n)
                    .collect(),
            )?
        } else {
            bands.clone()
        };
        let aperiodic_source = dsp::synthesis(&gated, &self.synthesis, budget)?;
        let noise_parameters = self.noise.run_with(&hidden, budget)?;
        let aperiodic = dsp::ltv(
            &noise_parameters,
            &aperiodic_source,
            self.spectral,
            effect(SpectrumKind::Aperiodic),
            budget,
        )?;
        let pcm = Tensor::new(
            harmonic.rows(),
            1,
            harmonic
                .values()
                .iter()
                .zip(aperiodic.values())
                .map(|(h, a)| h + a)
                .collect(),
        )?;
        Ok(Render {
            stages: BTreeMap::from([
                ("normalized", normalized),
                ("shape", shape),
                ("hidden", hidden),
                ("source", source),
                ("harmonic-parameters", harmonic_parameters),
                ("harmonic", harmonic),
                ("subbands", subbands),
                ("noise", noise),
                ("upsampled", upsampled),
                ("bands", bands),
                ("gated", gated),
                ("aperiodic-source", aperiodic_source),
                ("noise-parameters", noise_parameters),
                ("aperiodic", aperiodic),
                ("pcm", pcm),
            ]),
        })
    }
}
