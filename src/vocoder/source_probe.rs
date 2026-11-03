use super::{lf_excitation, lf_math::Mode};
use crate::{Result, graph::Budget, invalid, tensor::Tensor};

pub struct LfSource {
    pub cpu_flag: u8,
    pub values: Vec<f32>,
}

pub fn diagnostic_lf_source(
    voicing: &[f32],
    f0: &[f32],
    shapes: &[f32],
    gain: f32,
    flag: Option<u8>,
) -> Result<LfSource> {
    let frames = f0.len();
    if !(1..=256).contains(&frames)
        || voicing.len() != frames
        || shapes.len() != frames
        || voicing
            .iter()
            .chain(f0)
            .chain(shapes)
            .any(|value| !value.is_finite())
    {
        return Err(invalid(
            "LF source requires 1..256 finite aligned frame values",
        ));
    }
    let mode = match flag {
        Some(flag) => {
            Mode::from_flag(flag).ok_or_else(|| invalid("LF math flag must be 0, 1 or 3"))?
        }
        None => Mode::current(),
    };
    let mut values = vec![0.0; frames * 72];
    for (index, (&voicing, &f0)) in voicing.iter().zip(f0).enumerate() {
        values[index * 72] = voicing;
        values[index * 72 + 1] = f0;
    }
    let physical = Tensor::new(frames, 72, values)?;
    let shape = Tensor::new(frames, 1, shapes.to_vec())?;
    let source = lf_excitation::with_mode(&physical, &shape, gain, &mut Budget::default(), mode)?;
    Ok(LfSource {
        cpu_flag: mode.flag,
        values: source.values().to_vec(),
    })
}
