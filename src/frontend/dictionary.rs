use super::{bounded_read, u32_at};
use crate::{Result, invalid};
use std::path::Path;

const MAX_DICTIONARY: usize = 128 * 1024 * 1024;
const MAX_TOKENS: usize = 2_000_000;

#[derive(Clone, Copy)]
pub(super) struct Record {
    pub lc: u16,
    pub rc: u16,
    pub cost: i16,
    pub feature: usize,
}

pub(super) struct Dictionary {
    bytes: Vec<u8>,
    units: usize,
    token_start: usize,
    tokens: usize,
    feature_start: usize,
    left_ids: usize,
    right_ids: usize,
}

impl Dictionary {
    pub fn open(path: &Path, kind: u32) -> Result<Self> {
        let bytes = bounded_read(path, MAX_DICTIONARY)?;
        if bytes.len() < 80 || u32_at(&bytes, 0) ^ 0xef71_8f77 != bytes.len() as u32 {
            return Err(invalid("invalid dictionary size/magic"));
        }
        if u32_at(&bytes, 4) != 102 || u32_at(&bytes, 8) != kind {
            return Err(invalid("unsupported dictionary version/type"));
        }
        let charset = &bytes[40..72];
        let end = charset
            .iter()
            .position(|&c| c == 0)
            .ok_or_else(|| invalid("unterminated dictionary charset"))?;
        if !charset[..end].eq_ignore_ascii_case(b"utf-8") {
            return Err(invalid("dictionary must declare UTF-8"));
        }
        // Header lsize counts predecessor/right-context rows; rsize counts
        // successor/left-context rows. Token lc therefore uses rsize.
        let right_ids = u32_at(&bytes, 16) as usize;
        let left_ids = u32_at(&bytes, 20) as usize;
        if left_ids == 0 || right_ids == 0 || left_ids > 65_535 || right_ids > 65_535 {
            return Err(invalid("dictionary connection dimensions outside bounds"));
        }
        let trie_bytes = u32_at(&bytes, 24) as usize;
        let token_bytes = u32_at(&bytes, 28) as usize;
        let feature_bytes = u32_at(&bytes, 32) as usize;
        if trie_bytes == 0
            || trie_bytes % 8 != 0
            || token_bytes % 16 != 0
            || token_bytes / 16 > MAX_TOKENS
            || u32_at(&bytes, 12) as usize != token_bytes / 16
        {
            return Err(invalid("invalid dictionary table dimensions"));
        }
        let total = 72usize
            .checked_add(trie_bytes)
            .and_then(|n| n.checked_add(token_bytes))
            .and_then(|n| n.checked_add(feature_bytes))
            .ok_or_else(|| invalid("dictionary size overflow"))?;
        if total != bytes.len() || feature_bytes == 0 {
            return Err(invalid("dictionary section size mismatch"));
        }
        let result = Self {
            bytes,
            units: trie_bytes / 8,
            token_start: 72 + trie_bytes,
            tokens: token_bytes / 16,
            feature_start: 72 + trie_bytes + token_bytes,
            left_ids,
            right_ids,
        };
        let features = &result.bytes[result.feature_start..];
        let feature_text = std::str::from_utf8(features)
            .map_err(|_| invalid("dictionary features are not UTF-8"))?;
        if features.split(|&b| b == 0).any(|s| s.len() > 65_536) {
            return Err(invalid("dictionary feature length budget exceeded"));
        }
        if features.last() != Some(&0) {
            return Err(invalid("unterminated dictionary feature area"));
        }
        for index in 0..result.units {
            let (base, check) = result.unit(index)?;
            if base >= 0 && base as usize >= result.units {
                return Err(invalid("double-array base outside table"));
            }
            if check as usize >= result.units {
                return Err(invalid("double-array check outside table"));
            }
            if check as usize == index && base < 0 {
                result.range(terminal(base)?)?;
            }
        }
        for index in 0..result.tokens {
            let record = result.record(index);
            if record.lc as usize >= left_ids
                || record.rc as usize >= right_ids
                || record.feature >= features.len()
                || !feature_text.is_char_boundary(record.feature)
            {
                return Err(invalid("dictionary token index outside validated sections"));
            }
        }
        Ok(result)
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (self.left_ids, self.right_ids)
    }

    fn unit(&self, index: usize) -> Result<(i32, u32)> {
        if index >= self.units {
            return Err(invalid("double-array index outside table"));
        }
        let offset = 72 + index * 8;
        Ok((
            u32_at(&self.bytes, offset) as i32,
            u32_at(&self.bytes, offset + 4),
        ))
    }

    fn range(&self, value: u32) -> Result<std::ops::Range<usize>> {
        let start = (value >> 8) as usize;
        let count = (value & 255) as usize;
        if count == 0 || start.checked_add(count).is_none_or(|end| end > self.tokens) {
            return Err(invalid("double-array terminal token range outside table"));
        }
        Ok(start..start + count)
    }

    pub fn prefixes(&self, key: &[u8], steps: &mut usize) -> Result<Vec<(u32, usize)>> {
        let mut out = Vec::new();
        let initial = self.unit(0)?.0;
        if initial < 0 {
            return Err(invalid("negative double-array root"));
        }
        let mut base = initial as usize;
        for consumed in 0..=key.len() {
            super::lookup_step(steps)?;
            let (value, check) = self.unit(base)?;
            if check as usize == base && value < 0 {
                let value = terminal(value)?;
                self.range(value)?;
                out.push((value, consumed));
                if out.len() > 4096 {
                    return Err(invalid("dictionary prefix budget exceeded"));
                }
            }
            if consumed == key.len() {
                break;
            }
            let index = base
                .checked_add(key[consumed] as usize + 1)
                .ok_or_else(|| invalid("double-array offset overflow"))?;
            if index >= self.units {
                break;
            }
            let (next, check) = self.unit(index)?;
            if check as usize != base {
                break;
            }
            if next < 0 {
                return Err(invalid("negative double-array transition"));
            }
            base = next as usize;
        }
        Ok(out)
    }

    pub fn exact(&self, key: &[u8]) -> Result<Option<u32>> {
        Ok(self
            .prefixes(key, &mut 0)?
            .into_iter()
            .find_map(|(value, length)| (length == key.len()).then_some(value)))
    }

    fn record(&self, index: usize) -> Record {
        let offset = self.token_start + index * 16;
        let short = |at| u16::from_le_bytes(self.bytes[at..at + 2].try_into().unwrap());
        Record {
            lc: short(offset),
            rc: short(offset + 2),
            cost: short(offset + 6) as i16,
            feature: u32_at(&self.bytes, offset + 8) as usize,
        }
    }

    pub fn records(&self, value: u32) -> Result<Vec<Record>> {
        Ok(self.range(value)?.map(|index| self.record(index)).collect())
    }

    pub fn feature(&self, offset: usize) -> Result<&str> {
        let bytes = self
            .bytes
            .get(self.feature_start + offset..)
            .ok_or_else(|| invalid("feature offset outside dictionary"))?;
        let length = bytes
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| invalid("unterminated token feature"))?;
        std::str::from_utf8(&bytes[..length]).map_err(|_| invalid("token feature is not UTF-8"))
    }
}

fn terminal(base: i32) -> Result<u32> {
    base.checked_neg()
        .and_then(|n| n.checked_sub(1))
        .map(|n| n as u32)
        .ok_or_else(|| invalid("double-array terminal overflow"))
}
