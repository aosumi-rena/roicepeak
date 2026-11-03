use super::kernel::Admission;
use crate::graph::{
    Budget,
    matrix_storage::{Accumulate, Matrix, Workspace},
};
use crate::{Result, invalid, tensor::Tensor};

fn zeros(count: usize) -> Result<Vec<f32>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| invalid("attention workspace allocation failed"))?;
    values.resize(count, 0.0);
    Ok(values)
}

fn matrix(outputs: usize, inputs: usize, coefficients: Vec<f32>) -> Result<Matrix> {
    Matrix::Dense(Tensor::new(outputs, inputs, coefficients)?).into_product_kernel()
}

pub(super) fn run(
    q: &Tensor,
    k: &Tensor,
    v: &Tensor,
    heads: usize,
    budget: &mut Budget,
) -> Result<Tensor> {
    if heads == 0
        || q.cols() == 0
        || q.cols() != k.cols()
        || q.cols() != v.cols()
        || q.cols() % heads != 0
        || k.rows() != v.rows()
        || (q.rows() > 0 && k.rows() == 0)
    {
        return Err(invalid("attention projected dimensions"));
    }
    if q.values()
        .iter()
        .chain(k.values())
        .chain(v.values())
        .any(|v| !v.is_finite())
    {
        return Err(invalid("nonfinite attention projection"));
    }
    if q.rows() == 0 {
        return Tensor::zeros(0, q.cols());
    }
    let admission = Admission::modern()?;
    Admission::check_width(k.rows())?;
    let width = q.cols() / heads;
    if width == 0 || width > i32::MAX as usize || k.rows() > i32::MAX as usize {
        return Err(invalid("attention head dimensions"));
    }
    let key_stride = k.rows().div_ceil(8) * 8;
    let value_stride = width.div_ceil(8) * 8;
    let score_stride = k.rows().div_ceil(16) * 16;
    let key_coefficients = width.saturating_mul(key_stride);
    let value_coefficients = k.rows().saturating_mul(value_stride);
    let logical_coefficients = k.rows().saturating_mul(width);
    let scores_count = q.rows().saturating_mul(score_stride);
    let output_count = q.rows().saturating_mul(q.cols());
    let per_head_elements = logical_coefficients
        .saturating_mul(2)
        .saturating_add(key_coefficients)
        .saturating_add(value_coefficients)
        .saturating_add(scores_count);
    budget.spend(
        q.rows()
            .saturating_mul(key_coefficients.saturating_add(value_coefficients))
            .saturating_mul(heads),
        output_count.saturating_add(per_head_elements.saturating_mul(heads)),
    )?;
    let mut output = Tensor::zeros(q.rows(), q.cols())?;
    for head in 0..heads {
        let start = head * width;
        let mut coefficients = zeros(logical_coefficients)?;
        for source in 0..k.rows() {
            coefficients[source * width..(source + 1) * width]
                .copy_from_slice(&k.row(source)[start..start + width]);
        }
        let key = matrix(k.rows(), width, coefficients)?;
        let mut scores = zeros(scores_count)?;
        let mut workspace = Workspace::default();
        for (row, target) in scores.chunks_exact_mut(score_stride).enumerate() {
            key.product_into(
                &q.row(row)[start..start + width],
                &mut target[..k.rows()],
                Accumulate::Replace,
                &mut workspace,
            )?;
        }
        for target in scores.chunks_exact_mut(score_stride) {
            admission.scores(width, k.rows(), target)?;
        }
        drop(key);
        let mut coefficients = zeros(logical_coefficients)?;
        for channel in 0..width {
            for source in 0..v.rows() {
                coefficients[channel * v.rows() + source] = v.row(source)[start + channel];
            }
        }
        let value = matrix(width, v.rows(), coefficients)?;
        for (row, probabilities) in scores.chunks_exact(score_stride).enumerate() {
            value.product_into(
                &probabilities[..k.rows()],
                &mut output.row_mut(row)[start..start + width],
                Accumulate::Replace,
                &mut workspace,
            )?;
        }
    }
    if output.values().iter().any(|value| !value.is_finite()) {
        return Err(invalid("nonfinite attention context"));
    }
    Ok(output)
}
