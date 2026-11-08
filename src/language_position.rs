use crate::{Result, graph::Budget, invalid, tensor::Tensor};

pub(super) fn apply(input: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    let width = input.cols();
    if width % 2 != 0 {
        return Err(invalid("language embedding width must be even"));
    }
    let half = width / 2;
    let elements = input.values().len();
    budget.spend(elements.saturating_mul(3), elements + half)?;
    let denominators = (0..half)
        .map(|channel| 10000.0f32.powf((2 * channel) as f32 / width as f32))
        .collect::<Vec<_>>();
    let mut output = input.clone();
    for row in 0..input.rows() {
        let values = output.row_mut(row);
        for (channel, denominator) in denominators.iter().enumerate() {
            let angle = row as f32 / denominator;
            values[channel] = angle.sin() + values[channel];
            values[half + channel] = angle.cos() + values[half + channel];
        }
    }
    output.ensure_finite()?;
    Ok(output)
}
