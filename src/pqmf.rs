//! Eight-band FIR analysis and synthesis with ordered float32 accumulation.

use crate::{
    Result,
    graph::Budget,
    invalid,
    tensor::{Tensor, checked_elements},
};

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

fn analysis_range(
    signal: &Tensor,
    taps: &Tensor,
    first: usize,
    rows: usize,
    budget: &mut Budget,
) -> Result<Tensor> {
    check_taps(taps)?;
    if signal.cols() != 1 {
        return Err(invalid("PQMF analysis requires mono samples"));
    }
    signal.ensure_finite()?;
    let elements = checked_elements(rows, BANDS)?;
    budget.spend(rows.saturating_mul(BANDS * TAPS), elements)?;
    let mut output = vec![0.0f32; elements];
    for t in 0..rows {
        let end = (first + t + 1) * BANDS;
        for band in 0..BANDS {
            let mut sum = 0.0f32;
            for k in 0..TAPS {
                let at = end + k;
                if at >= TAPS && at - TAPS < signal.rows() {
                    let product = signal.row(at - TAPS)[0] * taps.row(band)[k];
                    sum = product + sum;
                }
            }
            output[t * BANDS + band] = sum;
        }
    }
    Tensor::new(rows, BANDS, output)
}

/// Fresh analysis processor output, including its leading and trailing rows.
pub fn analysis(signal: &Tensor, taps: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    let rows = signal
        .rows()
        .checked_add(DELAY)
        .ok_or_else(|| invalid("PQMF analysis length overflow"))?
        / BANDS;
    analysis_range(signal, taps, 0, rows, budget)
}

/// Analysis rows retained by the vocoder after its leading and EOF trimming.
pub fn vocoder_analysis(signal: &Tensor, taps: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    let raw_rows = signal
        .rows()
        .checked_add(DELAY)
        .ok_or_else(|| invalid("PQMF analysis length overflow"))?
        / BANDS;
    let rows = raw_rows.saturating_sub(LEADING_ROWS + 1);
    analysis_range(signal, taps, LEADING_ROWS, rows, budget)
}

/// Fresh synthesis processor output, including its initial delay behavior.
pub fn synthesis(bands: &Tensor, taps: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    check_taps(taps)?;
    if bands.cols() != BANDS {
        return Err(invalid("PQMF synthesis requires eight channels"));
    }
    bands.ensure_finite()?;
    let count = bands
        .rows()
        .checked_mul(BANDS)
        .ok_or_else(|| invalid("PQMF synthesis length overflow"))?;
    checked_elements(count, 1)?;
    budget.spend(bands.rows().saturating_mul(BANDS * TAPS), count)?;
    let mut output = vec![0.0f32; count];
    for t in 0..bands.rows() {
        let start = (t * BANDS).saturating_sub(DELAY);
        for band in 0..BANDS {
            let value = bands.row(t)[band];
            for k in 0..TAPS {
                let at = start + k;
                if at >= count {
                    break;
                }
                let product = taps.row(band)[k] * value;
                output[at] = product + output[at];
            }
        }
    }
    Tensor::new(count, 1, output)
}
