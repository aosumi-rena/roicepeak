use crate::{Result, invalid};

pub const MAX_ELEMENTS: usize = 16 * 1024 * 1024;
pub const MAX_FILE_BYTES: usize = MAX_ELEMENTS * 4 + 12;

/// Row-major float32, with time on rows and channels on columns.
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    rows: usize,
    cols: usize,
    values: Vec<f32>,
}

impl Tensor {
    /// Retained heap charge, excluding the inline Tensor itself.
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::values(&self.values)
    }

    pub fn new(rows: usize, cols: usize, values: Vec<f32>) -> Result<Self> {
        let n = checked_elements(rows, cols)?;
        if n != values.len() {
            return Err(invalid("tensor shape/data length mismatch"));
        }
        if values.iter().any(|x| !x.is_finite()) {
            return Err(invalid("non-finite tensor value"));
        }
        Ok(Self { rows, cols, values })
    }
    pub fn zeros(rows: usize, cols: usize) -> Result<Self> {
        Self::new(rows, cols, vec![0.0; checked_elements(rows, cols)?])
    }
    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn values(&self) -> &[f32] {
        &self.values
    }
    pub(crate) fn values_mut(&mut self) -> &mut [f32] {
        &mut self.values
    }
    pub fn row(&self, r: usize) -> &[f32] {
        &self.values[r * self.cols..(r + 1) * self.cols]
    }
    pub(crate) fn row_mut(&mut self, r: usize) -> &mut [f32] {
        &mut self.values[r * self.cols..(r + 1) * self.cols]
    }
    pub fn ensure_finite(&self) -> Result<()> {
        if self.values.iter().any(|x| !x.is_finite()) {
            Err(invalid("non-finite inference result"))
        } else {
            Ok(())
        }
    }
    pub fn from_rmat(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 12 || bytes.len() > MAX_FILE_BYTES || &bytes[..4] != b"RMT1" {
            return Err(invalid("invalid RMAT header or size"));
        }
        let rows = u32_at(bytes, 4)? as usize;
        let cols = u32_at(bytes, 8)? as usize;
        let n = checked_elements(rows, cols)?;
        if bytes.len() != 12 + 4 * n {
            return Err(invalid("RMAT exact length mismatch"));
        }
        Self::new(rows, cols, floats(&bytes[12..]))
    }
    pub fn to_rmat(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(12 + self.values.len() * 4);
        b.extend_from_slice(b"RMT1");
        b.extend_from_slice(&(self.rows as u32).to_le_bytes());
        b.extend_from_slice(&(self.cols as u32).to_le_bytes());
        for x in &self.values {
            b.extend_from_slice(&x.to_le_bytes());
        }
        b
    }
}

pub(crate) fn checked_elements(rows: usize, cols: usize) -> Result<usize> {
    if cols == 0 || rows > MAX_ELEMENTS || cols > MAX_ELEMENTS {
        return Err(invalid("invalid tensor dimensions"));
    }
    let n = rows
        .checked_mul(cols)
        .ok_or_else(|| invalid("tensor dimension overflow"))?;
    if n > MAX_ELEMENTS {
        return Err(invalid("tensor element budget exceeded"));
    }
    Ok(n)
}
pub(crate) fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    let end = at
        .checked_add(4)
        .ok_or_else(|| invalid("offset overflow"))?;
    let b = bytes.get(at..end).ok_or_else(|| invalid("truncated u32"))?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}
pub(crate) fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect()
}
