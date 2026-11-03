//! Checked time/channel assembly shared by speech stages.
use crate::{Result, graph::Budget, invalid, tensor::Tensor};

pub fn concatenate(parts: &[&Tensor], budget: &mut Budget) -> Result<Tensor> {
    let rows = parts
        .first()
        .ok_or_else(|| invalid("empty concatenation"))?
        .rows();
    let mut cols = 0usize;
    for p in parts {
        if p.rows() != rows {
            return Err(invalid("concatenation row counts differ"));
        }
        cols = cols
            .checked_add(p.cols())
            .ok_or_else(|| invalid("concatenation width overflow"))?;
    }
    budget.spend(0, rows.saturating_mul(cols))?;
    let mut out = Tensor::zeros(rows, cols)?;
    for t in 0..rows {
        let mut offset = 0;
        for p in parts {
            out.row_mut(t)[offset..offset + p.cols()].copy_from_slice(p.row(t));
            offset += p.cols();
        }
    }
    Ok(out)
}
pub fn broadcast(values: &[f32], rows: usize, budget: &mut Budget) -> Result<Tensor> {
    budget.spend(0, rows.saturating_mul(values.len()))?;
    let mut out = Tensor::zeros(rows, values.len())?;
    for t in 0..rows {
        out.row_mut(t).copy_from_slice(values);
    }
    out.ensure_finite()?;
    Ok(out)
}
pub fn repeat(input: &Tensor, counts: &[usize], budget: &mut Budget) -> Result<Tensor> {
    if input.rows() != counts.len() {
        return Err(invalid("duration row count differs"));
    }
    let rows = counts
        .iter()
        .try_fold(0usize, |n, c| n.checked_add(*c))
        .ok_or_else(|| invalid("duration overflow"))?;
    budget.spend(0, rows.saturating_mul(input.cols()))?;
    let mut out = Tensor::zeros(rows, input.cols())?;
    let mut t = 0;
    for (i, count) in counts.iter().enumerate() {
        for _ in 0..*count {
            out.row_mut(t).copy_from_slice(input.row(i));
            t += 1;
        }
    }
    Ok(out)
}
pub fn cosine_bank(values: &[f32], width: usize, budget: &mut Budget) -> Result<Tensor> {
    if width == 0 || values.iter().any(|v| !v.is_finite()) {
        return Err(invalid("cosine bank parameters"));
    }
    budget.spend(
        values.len().saturating_mul(width).saturating_mul(32),
        values.len().saturating_mul(width),
    )?;
    let mut out = Tensor::zeros(values.len(), width)?;
    for (t, v) in values.iter().enumerate() {
        for k in 0..width {
            let phase = (k as f32 - 1.0) / width as f32;
            let c = ((v - phase) * std::f32::consts::TAU).cos();
            out.row_mut(t)[k] = (f64::from(c) * 0.5 + 0.5) as f32;
        }
    }
    Ok(out)
}
pub fn positions(counts: &[usize]) -> Result<Vec<f32>> {
    let total = counts
        .iter()
        .try_fold(0usize, |n, c| n.checked_add(*c))
        .ok_or_else(|| invalid("duration overflow"))?;
    if total > crate::tensor::MAX_ELEMENTS {
        return Err(invalid("position allocation limit"));
    }
    let mut out = Vec::with_capacity(total);
    for n in counts {
        if *n == 1 {
            out.push(0.5);
        } else if *n > 1 {
            out.extend((0..*n).map(|i| i as f32 / (*n - 1) as f32));
        }
    }
    Ok(out)
}
