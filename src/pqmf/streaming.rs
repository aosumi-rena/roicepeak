use super::{BANDS, DELAY, LEADING_ROWS, TAPS, check_taps};
use crate::{
    Result,
    graph::Budget,
    invalid,
    tensor::{Tensor, checked_elements},
};
use std::collections::VecDeque;

pub(crate) struct Analysis {
    taps: Tensor,
    vocoder_trim: bool,
    history: VecDeque<f32>,
    history_start: usize,
    received: usize,
    next_row: usize,
    finished: bool,
}

impl Analysis {
    pub(crate) fn new(taps: &Tensor, vocoder_trim: bool) -> Result<Self> {
        check_taps(taps)?;
        Ok(Self {
            taps: taps.clone(),
            vocoder_trim,
            history: VecDeque::with_capacity(TAPS + BANDS),
            history_start: 0,
            received: 0,
            next_row: 0,
            finished: false,
        })
    }

    pub(crate) fn feed(
        &mut self,
        signal: &Tensor,
        end: bool,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if self.finished {
            return Err(invalid("PQMF analysis stream already finished"));
        }
        if signal.cols() != 1 {
            return Err(invalid("PQMF analysis requires mono samples"));
        }
        signal.ensure_finite()?;
        let received = self
            .received
            .checked_add(signal.rows())
            .ok_or_else(|| invalid("PQMF analysis length overflow"))?;
        let raw_rows = received
            .checked_add(if end { DELAY } else { 0 })
            .ok_or_else(|| invalid("PQMF analysis length overflow"))?
            / BANDS;
        let first = if self.vocoder_trim {
            self.next_row.max(LEADING_ROWS)
        } else {
            self.next_row
        };
        let last = raw_rows.saturating_sub(usize::from(self.vocoder_trim && end));
        let rows = last.saturating_sub(first);
        let elements = checked_elements(rows, BANDS)?;
        budget.spend(rows.saturating_mul(BANDS * TAPS), elements)?;
        let mut output = Vec::with_capacity(elements);
        for &sample in signal.values() {
            self.history.push_back(sample);
            self.received += 1;
            if self.received % BANDS == 0 {
                self.row(last, &mut output)?;
            }
        }
        while self.next_row < raw_rows {
            self.row(last, &mut output)?;
        }
        self.finished = end;
        Tensor::new(rows, BANDS, output)
    }

    fn row(&mut self, last: usize, output: &mut Vec<f32>) -> Result<()> {
        let end = self
            .next_row
            .checked_add(1)
            .and_then(|row| row.checked_mul(BANDS))
            .ok_or_else(|| invalid("PQMF analysis length overflow"))?;
        if (!self.vocoder_trim || self.next_row >= LEADING_ROWS) && self.next_row < last {
            for band in 0..BANDS {
                let mut sum = 0.0f32;
                for k in 0..TAPS {
                    let at = end
                        .checked_add(k)
                        .ok_or_else(|| invalid("PQMF analysis length overflow"))?;
                    if at >= TAPS && at - TAPS < self.received {
                        let index = (at - TAPS)
                            .checked_sub(self.history_start)
                            .ok_or_else(|| invalid("PQMF analysis history is unavailable"))?;
                        let sample = *self
                            .history
                            .get(index)
                            .ok_or_else(|| invalid("PQMF analysis history is unavailable"))?;
                        let product = sample * self.taps.row(band)[k];
                        sum = product + sum;
                    }
                }
                output.push(sum);
            }
        }
        self.next_row += 1;
        let keep = end
            .checked_add(BANDS)
            .ok_or_else(|| invalid("PQMF analysis length overflow"))?
            .saturating_sub(TAPS)
            .min(self.received);
        while self.history_start < keep {
            self.history.pop_front();
            self.history_start += 1;
        }
        Ok(())
    }
}

pub(crate) struct Synthesis {
    taps: Tensor,
    overlap: VecDeque<f32>,
    rows: usize,
    emitted: usize,
    finished: bool,
}

impl Synthesis {
    pub(crate) fn new(taps: &Tensor) -> Result<Self> {
        check_taps(taps)?;
        Ok(Self {
            taps: taps.clone(),
            overlap: VecDeque::with_capacity(TAPS),
            rows: 0,
            emitted: 0,
            finished: false,
        })
    }

    pub(crate) fn feed(
        &mut self,
        bands: &Tensor,
        end: bool,
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if self.finished {
            return Err(invalid("PQMF synthesis stream already finished"));
        }
        if bands.cols() != BANDS {
            return Err(invalid("PQMF synthesis requires eight channels"));
        }
        bands.ensure_finite()?;
        let rows = self
            .rows
            .checked_add(bands.rows())
            .ok_or_else(|| invalid("PQMF synthesis length overflow"))?;
        let target = rows
            .checked_mul(BANDS)
            .ok_or_else(|| invalid("PQMF synthesis length overflow"))?;
        let release = if end {
            target
        } else {
            target.saturating_sub(DELAY)
        };
        let count = release
            .checked_sub(self.emitted)
            .ok_or_else(|| invalid("PQMF synthesis output cursor"))?;
        checked_elements(count, 1)?;
        budget.spend(bands.rows().saturating_mul(BANDS * TAPS), count)?;
        let mut output = Vec::with_capacity(count);
        for row in 0..bands.rows() {
            let start = self
                .rows
                .checked_mul(BANDS)
                .ok_or_else(|| invalid("PQMF synthesis length overflow"))?
                .saturating_sub(DELAY);
            let needed = start
                .checked_add(TAPS)
                .and_then(|last| last.checked_sub(self.emitted))
                .ok_or_else(|| invalid("PQMF synthesis length overflow"))?;
            self.overlap.resize(needed, 0.0);
            for band in 0..BANDS {
                let value = bands.row(row)[band];
                for k in 0..TAPS {
                    let at = start + k;
                    if end && at >= target {
                        break;
                    }
                    let product = self.taps.row(band)[k] * value;
                    let existing = &mut self.overlap[at - self.emitted];
                    *existing = product + *existing;
                }
            }
            self.rows += 1;
            self.release((self.rows * BANDS).saturating_sub(DELAY), &mut output)?;
        }
        if end {
            self.release(target, &mut output)?;
            self.overlap.clear();
        }
        self.finished = end;
        Tensor::new(count, 1, output)
    }

    fn release(&mut self, end: usize, output: &mut Vec<f32>) -> Result<()> {
        while self.emitted < end {
            output.push(
                self.overlap
                    .pop_front()
                    .ok_or_else(|| invalid("PQMF synthesis overlap is unavailable"))?,
            );
            self.emitted += 1;
        }
        Ok(())
    }
}
