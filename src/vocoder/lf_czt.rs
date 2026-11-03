use super::{complex_fft, lf_trig};
use crate::{Result, invalid};

pub(super) const LENGTH: usize = 480;
pub(super) const WORDS: usize = 2048;

pub(super) struct Trace {
    pub packed: Vec<f32>,
    pub input_fft: Vec<f32>,
    pub kernel_fft: Vec<f32>,
    pub product: Vec<f32>,
    pub inverse: Vec<f32>,
}

pub(super) struct Transform {
    chirp: [f32; WORDS],
    kernel: [f32; WORDS],
    fused: bool,
}

impl Transform {
    pub fn from_angular_step(angular_step: f32, fused: bool) -> Result<Self> {
        if !angular_step.is_finite() {
            return Err(invalid("LF chirp angular step must be finite"));
        }
        let arguments = chirp_arguments(angular_step);
        if arguments
            .iter()
            .any(|value| !value.is_finite() || value.abs() > 361_000.0)
        {
            return Err(invalid("LF chirp argument exceeds its physical range"));
        }
        let mut chirp = [0.0; WORDS];
        chirp[0] = 1.0;
        for (i, argument) in arguments.iter().enumerate().skip(1) {
            let (sine, cosine) = lf_trig::sine_cosine(*argument, fused);
            let imaginary = f32::from_bits(sine.to_bits() ^ 0x8000_0000);
            for index in [i, WORDS / 2 - i] {
                chirp[2 * index] = cosine;
                chirp[2 * index + 1] = imaginary;
            }
        }
        Ok(Self::from_chirp(chirp, fused))
    }

    pub fn chirp(&self) -> &[f32; WORDS] {
        &self.chirp
    }

    pub fn from_chirp(chirp: [f32; WORDS], fused: bool) -> Self {
        let mut kernel = chirp;
        complex_fft::transform(&mut kernel, true, fused);
        Self {
            chirp,
            kernel,
            fused,
        }
    }

    pub fn apply(
        &self,
        real: &[f32; LENGTH],
        imaginary: &[f32; LENGTH],
        work: &mut [f32; WORDS],
        mut trace: Option<&mut Trace>,
    ) -> ([f32; LENGTH], [f32; LENGTH]) {
        work.fill(0.0);
        work[0] = real[0];
        work[1] = imaginary[0];
        for i in 1..LENGTH {
            let (ar, ai) = (real[i], imaginary[i]);
            let (br, bi) = (self.chirp[2 * i], self.chirp[2 * i + 1]);
            work[2 * i] = ar * br + ai * bi;
            work[2 * i + 1] = ai * br - ar * bi;
        }
        if let Some(trace) = trace.as_deref_mut() {
            trace.packed = work.to_vec();
        }
        complex_fft::transform(work, true, self.fused);
        if let Some(trace) = trace.as_deref_mut() {
            trace.input_fft = work.to_vec();
            trace.kernel_fft = self.kernel.to_vec();
        }
        for i in 0..WORDS / 2 {
            let (ar, ai) = (work[2 * i], work[2 * i + 1]);
            let (br, bi) = (self.kernel[2 * i], self.kernel[2 * i + 1]);
            work[2 * i] = ar * br - ai * bi;
            work[2 * i + 1] = ar * bi + ai * br;
        }
        if let Some(trace) = trace.as_deref_mut() {
            trace.product = work.to_vec();
        }
        complex_fft::transform(work, false, self.fused);
        if let Some(trace) = trace {
            trace.inverse = work.to_vec();
        }
        let mut real = [0.0; LENGTH];
        let mut imaginary = [0.0; LENGTH];
        for i in 0..LENGTH {
            let (ar, ai) = (work[2 * i], work[2 * i + 1]);
            let (br, bi) = (self.chirp[2 * i], self.chirp[2 * i + 1]);
            real[i] = (ar * br + ai * bi) * (1.0 / 1024.0);
            imaginary[i] = (ai * br - ar * bi) * (1.0 / 1024.0);
        }
        (real, imaginary)
    }
}

pub(super) fn chirp_arguments(angular_step: f32) -> [f32; LENGTH] {
    let factor = f64::from(angular_step) * -0.5;
    std::array::from_fn(|i| {
        let i = i as f64;
        ((i * i) * factor) as f32
    })
}
