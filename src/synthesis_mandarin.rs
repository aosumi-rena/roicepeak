use crate::{phonemes::Encoded, tensor::Tensor};
use std::ops::Range;

pub(super) fn shorten_neutral(
    encoded: &Encoded,
    language: usize,
    period: f32,
    durations: &mut [f32],
) {
    let period = f64::from(period);
    let mut first = 0;
    let mut preceding_tone = false;
    for group in encoded.references.chunk_by(|a, b| a[3] == b[3]) {
        let end = first + group.len();
        let nucleus = (first..end).find(|&index| {
            encoded.nucleus[index] && encoded.linguistic.row(index)[1] == language as f32
        });
        match nucleus.map(|index| (index, encoded.codes[index])) {
            Some((_, 0x3001..=0x3004)) => preceding_tone = true,
            Some((index, 0x3000)) if preceding_tone => {
                for duration in &mut durations[index..end] {
                    let frames = (f64::from(*duration) / period).round();
                    if frames > 1.0 {
                        *duration = ((frames * 2.0 / 3.0).round() * period) as f32;
                    }
                }
            }
            _ => preceding_tone = false,
        }
        first = end;
    }
}

struct Syllable {
    frames: Range<usize>,
    tone: u32,
}

pub(super) struct Contour {
    syllables: Vec<Syllable>,
    next: usize,
    frame: usize,
    anchor: Option<f32>,
    previous_end: Option<f32>,
    last_pitch: Option<f32>,
}

impl Contour {
    pub(super) fn new(
        encoded: &Encoded,
        prosody: &Tensor,
        controls: &Tensor,
        mandarin: Option<usize>,
        first_frame: usize,
    ) -> Self {
        let mut syllables = Vec::new();
        if let Some(language) = mandarin
            && encoded
                .codes
                .iter()
                .any(|&code| matches!(code, 0x3001..=0x3004))
        {
            let mut phone = 0;
            let mut frame = 0;
            for group in encoded.references.chunk_by(|a, b| a[3] == b[3]) {
                let end_phone = phone + group.len();
                let start = frame;
                let mut tone = 0;
                for index in phone..end_phone {
                    frame += controls.row(index)[2] as usize;
                    if encoded.nucleus[index] && encoded.linguistic.row(index)[1] == language as f32
                    {
                        tone = prosody.row(index)[0] as u32;
                    }
                }
                if start < frame && matches!(tone, 0x3001..=0x3004) {
                    syllables.push(Syllable {
                        frames: start..frame,
                        tone,
                    });
                }
                phone = end_phone;
            }
        }
        let next = syllables.partition_point(|part| part.frames.end <= first_frame);
        Self {
            syllables,
            next,
            frame: first_frame,
            anchor: None,
            previous_end: None,
            last_pitch: None,
        }
    }

    pub(super) fn seek_start(&self) -> usize {
        let Some(part) = self.syllables.get(self.next) else {
            return self.frame;
        };
        if self.frame < part.frames.start {
            return self.frame;
        }
        self.next
            .checked_sub(1)
            .and_then(|previous| self.syllables.get(previous))
            .filter(|previous| previous.frames.end == part.frames.start)
            .map_or(part.frames.start, |previous| previous.frames.start)
    }

    pub(super) fn apply(&mut self, physical: &mut [f32], force_voiced: bool) {
        if self.next == self.syllables.len() {
            return;
        }
        let first = self.frame;
        self.frame += physical.len() / 72;
        for (index, row) in physical.chunks_exact_mut(72).enumerate() {
            let frame = first + index;
            let Some(part) = self.syllables.get(self.next) else {
                break;
            };
            if frame < part.frames.start {
                continue;
            }
            let following = self
                .syllables
                .get(self.next + 1)
                .filter(|next| next.frames.start == part.frames.end);
            let half_third =
                following.is_some_and(|next| matches!(next.tone, 0x3001 | 0x3002 | 0x3004));
            if row[1] > 0.0 {
                let raw = row[1].log2();
                let anchor = *self.anchor.get_or_insert(raw);
                if force_voiced || row[0] >= 0.5 {
                    let phase = (frame - part.frames.start) as f32 / part.frames.len() as f32;
                    let target = anchor
                        + (raw - anchor) * 0.25
                        + semitones(part.tone, phase, half_third) / 12.0;
                    let from = self.previous_end.unwrap_or(raw);
                    let onset = smooth(phase / 0.22);
                    let pitch = if phase < 0.22 {
                        from + (target - from) * onset
                    } else {
                        target
                    };
                    let ending = if following.is_some() {
                        1.0
                    } else {
                        smooth((1.0 - phase) / 0.19)
                    };
                    let corrected = raw + (pitch - raw) * ending;
                    row[1] = corrected.exp2();
                    self.last_pitch = Some(corrected);
                } else {
                    self.last_pitch = None;
                }
            }
            if frame + 1 == part.frames.end {
                self.previous_end = if following.is_some() {
                    self.last_pitch
                } else {
                    None
                };
                self.anchor = None;
                self.last_pitch = None;
                self.next += 1;
            }
        }
    }
}

fn semitones(tone: u32, phase: f32, half_third: bool) -> f32 {
    match tone {
        0x3001 => 3.0,
        0x3002 => -1.5 + 4.5 * phase,
        0x3003 if phase <= 0.5 => -1.5 - 3.0 * phase,
        0x3003 if half_third => -3.0,
        0x3003 => -3.0 + 7.0 * (phase - 0.5),
        0x3004 => 3.0 - 6.0 * phase,
        _ => 0.0,
    }
}

fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
