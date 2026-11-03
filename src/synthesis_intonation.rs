use crate::{phonemes::Encoded, prosody::logit, stylebook::Style, tensor::Tensor};

pub(super) fn coordinates(
    x: &Encoded,
    predicted: &Tensor,
    style: &Style,
    invert: bool,
) -> Vec<f32> {
    let slots = x.references.iter().map(|r| r[3]).max().unwrap_or(0) + 1;
    let mut user = vec![0.5; slots];
    for (i, r) in x.references.iter().enumerate() {
        if x.nucleus[i] {
            let value = 1.0 / (1.0 + (-(logit(predicted.row(i)[1]) + style.intonation)).exp());
            user[r[3]] = if invert { 1.0 - value } else { value };
        }
    }
    let mean = user.iter().map(|v| f64::from(*v)).sum::<f64>() / slots as f64;
    let variance = user
        .iter()
        .map(|v| (f64::from(*v) - mean).powi(2))
        .sum::<f64>()
        / slots as f64;
    if variance > 0.0 {
        let deviation = variance.sqrt();
        for i in 1..slots.saturating_sub(1) {
            user[i] = ((f64::from(user[i]) - mean) / deviation) as f32;
        }
    } else {
        user.fill(0.0);
    }
    user
}
