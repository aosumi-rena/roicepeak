//! Character sequences and explicit zero rows before the word encoder.
use crate::{Result, graph::Budget, invalid, tensor::Tensor};
use std::collections::HashMap;

pub(crate) fn ordinary(word: &str, alphabet: &HashMap<char, usize>) -> Vec<usize> {
    std::iter::once(1)
        .chain(
            word.chars()
                .map(|ch| alphabet.get(&ch).copied().unwrap_or(0)),
        )
        .chain(std::iter::once(2))
        .chain(std::iter::repeat_n(0, 7))
        .collect()
}

pub(crate) fn special(role: u32) -> Result<Vec<usize>> {
    let mut ids = match role {
        0 | 2 => vec![0; 12],
        1 => vec![0; 14],
        _ => return Err(invalid("unsupported character boundary role")),
    };
    ids[0] = role as usize;
    Ok(ids)
}

pub(crate) fn embed(ids: &[usize], table: &Tensor, budget: &mut Budget) -> Result<Tensor> {
    let mut embedded = Tensor::zeros(ids.len(), table.rows())?;
    budget.spend(0, embedded.values().len())?;
    for (row, id) in ids.iter().copied().enumerate() {
        if id == 0 {
            continue;
        }
        if id >= table.cols() {
            return Err(invalid("character ID outside table"));
        }
        for channel in 0..table.rows() {
            embedded.row_mut(row)[channel] = table.row(channel)[id];
        }
    }
    Ok(embedded)
}
