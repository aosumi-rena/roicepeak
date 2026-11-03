use super::{DurationEdit, Edits};
use crate::{Result, invalid, phonemes::Encoded, stylebook::Style, tensor::Tensor};

pub(super) fn raw(x: &Encoded, predicted: &Tensor, style: &Style, period: f32) -> Result<Vec<f32>> {
    let n = x.linguistic.rows();
    if predicted.rows() != n || predicted.cols() != 3 || !period.is_finite() || period <= 0.0 {
        return Err(invalid("prosody control dimensions"));
    }
    (0..n)
        .map(|t| {
            let name = if x.nucleus[t] {
                "##nuc##"
            } else if t == 0 && style.phoneme_log_speeds.contains_key("##bos##") {
                "##bos##"
            } else if t + 1 == n && style.phoneme_log_speeds.contains_key("##eos##") {
                "##eos##"
            } else {
                &x.symbols[t]
            };
            let speed = style
                .phoneme_log_speeds
                .get(name)
                .copied()
                .unwrap_or(style.log_speed)
                .exp()
                .max(0.01);
            let count = (f64::from(predicted.row(t)[2] / speed) + 0.5).floor();
            let seconds = count as f32 * period;
            if !seconds.is_finite() || seconds < 0.0 {
                return Err(invalid("invalid raw query duration"));
            }
            Ok(seconds)
        })
        .collect()
}

pub(super) struct Plan {
    pub(super) frames: Vec<f32>,
    pub(super) seconds: Vec<f32>,
}

pub(super) fn plan(
    x: &Encoded,
    raw: &[f32],
    period: f32,
    edits: &Edits,
    speed_factor: f64,
    pause_factor: f64,
) -> Result<Plan> {
    let n = x.linguistic.rows();
    if raw.len() != n || !period.is_finite() || period <= 0.0 {
        return Err(invalid("raw duration dimensions or frame period"));
    }
    if edits.authored_duration.iter().any(|(i, edit)| {
        *i >= n || edits.duration.contains_key(i) || !edit.value().is_finite() || edit.value() < 0.0
    }) {
        return Err(invalid("invalid or overlapping authored duration edit"));
    }
    let mut frames = Vec::with_capacity(n);
    let mut controlled_seconds = Vec::with_capacity(n);
    for (t, raw) in raw.iter().copied().enumerate() {
        let mut seconds = f64::from(raw);
        let authored = edits.authored_duration.get(&t);
        if let Some(DurationEdit::Absolute(value)) = authored {
            seconds = f64::from((*value as f32).max(0.01));
        } else {
            if t > 0 && t + 1 < n {
                if x.nucleus[t] {
                    let scaled = (seconds * speed_factor) as f32;
                    if !scaled.is_finite() {
                        return Err(invalid("speed-scaled duration overflow"));
                    }
                    seconds = f64::from(scaled);
                } else if ["pau", "sil"].contains(&x.symbols[t].as_str()) {
                    let scaled = (seconds * pause_factor) as f32;
                    if !scaled.is_finite() {
                        return Err(invalid("pause-scaled duration overflow"));
                    }
                    seconds = f64::from(scaled.max(0.01));
                }
            }
            if let Some(DurationEdit::Relative(value)) = authored {
                let scaled = (seconds * *value) as f32;
                seconds = f64::from(if *value < 1.0 {
                    scaled.max(0.01)
                } else {
                    scaled
                });
            }
        }
        let count = (seconds.max(0.0) / f64::from(period) + 0.5).floor();
        if !seconds.is_finite() || !count.is_finite() || count > 1_000_000.0 {
            return Err(invalid("scaled duration exceeds limit"));
        }
        frames.push(count as f32);
        controlled_seconds.push(seconds as f32);
    }
    Ok(Plan {
        frames,
        seconds: controlled_seconds,
    })
}
