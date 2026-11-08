//! Eight-band FIR analysis and synthesis with ordered float32 accumulation.

use crate::{Result, graph::Budget, invalid, tensor::Tensor};

mod streaming;
pub(crate) use streaming::{Analysis, Synthesis};

pub const BANDS: usize = 8;
pub const TAPS: usize = 65;
const DELAY: usize = TAPS / 2;
const LEADING_ROWS: usize = BANDS / 2 - 1;

fn check_taps(taps: &Tensor) -> Result<()> {
    if taps.rows() != BANDS || taps.cols() != TAPS {
        return Err(invalid("PQMF requires eight 65-tap filters"));
    }
    taps.ensure_finite()
}

/// Fresh analysis processor output, including its leading and trailing rows.
pub fn analysis(signal: &Tensor, taps: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    Analysis::new(taps, false)?.feed(signal, true, budget)
}

/// Analysis rows retained by the vocoder after its leading and EOF trimming.
pub fn vocoder_analysis(signal: &Tensor, taps: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    Analysis::new(taps, true)?.feed(signal, true, budget)
}

/// Fresh synthesis processor output, including its initial delay behavior.
pub fn synthesis(bands: &Tensor, taps: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    Synthesis::new(taps)?.feed(bands, true, budget)
}
