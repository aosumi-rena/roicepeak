use super::super::{dsp::HOP, lf_czt, lf_harmonics, lf_math::Mode, lf_spectrum, lf_window};
use crate::{Result, graph::Budget, invalid, tensor::Tensor};

const PERIOD: f32 = 0.005;

pub(in crate::vocoder) struct Stream {
    mode: Mode,
    force_voiced: bool,
    window: [f32; lf_window::LENGTH],
    phase: f64,
    overlap: [f32; lf_window::LENGTH],
    frequencies: Vec<f32>,
    workspace: [f32; lf_czt::WORDS],
    transform: Option<(u32, lf_czt::Transform)>,
    started: bool,
    finished: bool,
}

impl Stream {
    pub(in crate::vocoder) fn new(gain: f32, force_voiced: bool, mode: Mode) -> Result<Self> {
        if !gain.is_finite() {
            return Err(invalid("excitation dimensions or gain"));
        }
        Ok(Self {
            mode,
            force_voiced,
            window: lf_window::window(gain, mode.fused),
            phase: 0.0,
            overlap: [0.0; lf_window::LENGTH],
            frequencies: Vec::with_capacity(lf_harmonics::MAX_HARMONICS),
            workspace: [0.0; lf_czt::WORDS],
            transform: None,
            started: false,
            finished: false,
        })
    }

    pub(in crate::vocoder) fn feed(
        &mut self,
        physical: &Tensor,
        shape: &Tensor,
        end: bool,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if self.finished {
            return Err(invalid("excitation stream already finished"));
        }
        if physical.cols() != 72 || shape.cols() != 1 || shape.rows() != physical.rows() {
            return Err(invalid("excitation dimensions or gain"));
        }
        let frames = physical.rows();
        let mut hops = frames;
        if !self.started && hops != 0 {
            hops -= 1;
        }
        if end && (self.started || frames != 0) {
            hops = hops
                .checked_add(1)
                .ok_or_else(|| invalid("excitation length overflow"))?;
        }
        let count = hops
            .checked_mul(HOP)
            .ok_or_else(|| invalid("excitation length overflow"))?;
        budget.spend(frames.saturating_mul(150_000), count)?;
        let mut out = Vec::with_capacity(count);
        for t in 0..frames {
            self.accumulate(physical.row(t), shape.row(t)[0])?;
            if self.started {
                out.extend_from_slice(&self.overlap[..HOP]);
            }
            self.started = true;
            self.overlap.copy_within(HOP.., 0);
            self.overlap[HOP..].fill(0.0);
        }
        if end {
            if self.started {
                out.extend_from_slice(&self.overlap[..HOP]);
            }
            self.finished = true;
        }
        Tensor::new(count, 1, out)
    }

    fn accumulate(&mut self, physical: &[f32], shape: f32) -> Result<()> {
        let f0 = physical[1];
        let step = lf_harmonics::advance_phase(self.phase, PERIOD, f0)?;
        self.phase = step.after;
        if !self.force_voiced && physical[0] < 0.5 {
            return Ok(());
        }
        let sample_rate = HOP as f32 / PERIOD;
        let harmonics = lf_harmonics::harmonic_count(sample_rate, f0)?;
        if harmonics == 0 {
            return Ok(());
        }
        self.frequencies.clear();
        self.frequencies
            .extend((1..=harmonics).map(|harmonic| harmonic as f32 * 100.0));
        let spectrum = lf_spectrum::Prepared::new(shape, 100.0, self.mode.fused)?;
        let (amplitudes, phases) =
            spectrum.amplitude_phase(&self.frequencies, self.mode.alternate)?;
        let packed = lf_harmonics::pack_fraction(
            sample_rate,
            f0,
            PERIOD,
            step.fraction,
            &amplitudes,
            &phases,
            self.mode.fused,
            false,
        )?;
        if self.transform.as_ref().map(|(key, _)| *key) != Some(packed.angular_step.to_bits()) {
            self.transform = Some((
                packed.angular_step.to_bits(),
                lf_czt::Transform::from_angular_step(packed.angular_step, self.mode.fused)?,
            ));
        }
        let transform = &self
            .transform
            .as_ref()
            .expect("LF transform was prepared")
            .1;
        let (source, _) =
            transform.apply(&packed.real, &packed.imaginary, &mut self.workspace, None);
        for (i, source) in source.into_iter().enumerate() {
            let value = lf_window::accumulate(source, self.window[i], self.overlap[i]);
            if !value.is_finite() {
                return Err(invalid("LF excitation produced non-finite values"));
            }
            self.overlap[i] = value;
        }
        Ok(())
    }
}
