//! Block offset modes and sentence placement.
use crate::app::document::{Block, OffsetMode};

const SAMPLE_RATE: f64 = 48_000.0;

pub struct SentencePlacement {
    pub start: f64,
    pub duration: f64,
    pub sample_start: usize,
}

pub struct BlockPlacement {
    pub start: f64,
    pub duration: f64,
    pub end: f64,
    pub sentences: Vec<SentencePlacement>,
}

pub struct Planner {
    previous: Option<(f64, f64)>,
    maximum_end: f64,
    max_samples: usize,
}

impl Planner {
    pub fn new(max_samples: usize) -> Self {
        Self {
            previous: None,
            maximum_end: 0.0,
            max_samples,
        }
    }

    /// Empty/unprepared blocks do not replace the preceding prepared block.
    /// Sentence durations stay in model order, including their boundary silence.
    pub fn append(
        &mut self,
        block: &Block,
        sentence_durations: &[f64],
    ) -> Result<BlockPlacement, String> {
        if !block.offset_seconds.is_finite() {
            return Err("Block offset must be finite".into());
        }
        let reference = match (block.offset_mode, self.previous) {
            (OffsetMode::PreviousStart, Some((start, _))) => start,
            (OffsetMode::PreviousEnd, Some((start, duration))) => start + duration,
            _ => 0.0,
        };
        let start = reference + block.offset_seconds;
        if !start.is_finite() {
            return Err("Block placement overflow".into());
        }
        let start = start.max(0.0);
        sample_index(start, self.max_samples)?;
        let mut cursor = start;
        let mut duration = 0.0;
        let mut sentences = Vec::with_capacity(sentence_durations.len());
        for &seconds in sentence_durations {
            if !seconds.is_finite() || seconds < 0.0 {
                return Err("Sentence duration must be finite and nonnegative".into());
            }
            sentences.push(SentencePlacement {
                start: cursor,
                duration: seconds,
                sample_start: sample_index(cursor, self.max_samples)?,
            });
            cursor += seconds;
            duration += seconds;
            sample_index(cursor, self.max_samples)?;
        }
        if !sentences.is_empty() {
            self.previous = Some((start, duration));
            self.maximum_end = self.maximum_end.max(cursor);
        }
        Ok(BlockPlacement {
            start,
            duration,
            end: cursor,
            sentences,
        })
    }

    pub fn sample_count(&self) -> Result<usize, String> {
        sample_index(self.maximum_end, self.max_samples)
    }
}

fn sample_index(seconds: f64, max_samples: usize) -> Result<usize, String> {
    let samples = seconds * SAMPLE_RATE;
    if !samples.is_finite() || samples < 0.0 || samples > max_samples as f64 {
        return Err("Project placement exceeds the synthesis sample budget".into());
    }
    // Truncate nonnegative sample positions towards zero.
    Ok(samples.trunc() as usize)
}

/// Mix in original block/sentence order using f32 addition.
/// The separately truncated project duration clips the final sample.
pub fn mix(output: &mut [f32], sample_start: usize, pcm: &[f32]) -> Result<(), String> {
    if sample_start > output.len() {
        return Err("Sentence starts outside the project audio buffer".into());
    }
    for (destination, source) in output[sample_start..].iter_mut().zip(pcm) {
        let value = *destination + *source;
        if !value.is_finite() {
            return Err("Non-finite mixed speech sample".into());
        }
        *destination = value;
    }
    Ok(())
}
