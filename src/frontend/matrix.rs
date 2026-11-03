use super::bounded_read;
use crate::{Result, invalid};
use std::path::Path;

const RANK: usize = 400;

pub(super) struct Matrix {
    previous: Vec<f32>,
    next: Vec<f32>,
    previous_rows: usize,
    next_rows: usize,
}

impl Matrix {
    pub fn open(dic: &Path, previous_rows: usize, next_rows: usize) -> Result<Self> {
        Ok(Self {
            previous: factor(&dic.join("matrix.bin.400.vhti"), previous_rows)?,
            next: factor(&dic.join("matrix.bin.400.usi"), next_rows)?,
            previous_rows,
            next_rows,
        })
    }

    pub fn cost(&self, previous: u16, next: u16) -> Result<i64> {
        let previous = previous as usize;
        let next = next as usize;
        if previous >= self.previous_rows || next >= self.next_rows {
            return Err(invalid("connection ID outside factor matrix"));
        }
        let left = &self.previous[previous * RANK..(previous + 1) * RANK];
        let right = &self.next[next * RANK..(next + 1) * RANK];
        let dot = left
            .iter()
            .zip(right)
            .map(|(&a, &b)| f64::from(a * b))
            .sum::<f64>() as f32;
        let rounded = dot.round_ties_even();
        // Avoid saturating casts and leave headroom for checked path additions.
        if !rounded.is_finite() || rounded.abs() > 1_000_000_000_000.0 {
            return Err(invalid("nonfinite or excessive connection cost"));
        }
        Ok(rounded as i64)
    }
}

fn factor(path: &Path, expected_rows: usize) -> Result<Vec<f32>> {
    let bytes = bounded_read(path, 4 + 65_535 * (4 + RANK * 2))?;
    if bytes.len() < 4 {
        return Err(invalid("truncated connection factor"));
    }
    let rows = u16::from_le_bytes(bytes[..2].try_into().unwrap()) as usize;
    let rank = u16::from_le_bytes(bytes[2..4].try_into().unwrap()) as usize;
    if rows != expected_rows || rank != RANK || bytes.len() != 4 + rows * (4 + rank * 2) {
        return Err(invalid(
            "connection factor dimensions do not match dictionary",
        ));
    }
    let mut result = Vec::with_capacity(rows * rank);
    for row in bytes[4..].chunks_exact(4 + rank * 2) {
        let scale = f32::from_le_bytes(row[..4].try_into().unwrap());
        if !scale.is_finite() {
            return Err(invalid("nonfinite connection scale"));
        }
        for item in row[4..].chunks_exact(2) {
            let value = f32::from(i16::from_le_bytes(item.try_into().unwrap())) * scale;
            if !value.is_finite() {
                return Err(invalid("nonfinite expanded connection factor"));
            }
            result.push(value);
        }
    }
    Ok(result)
}
