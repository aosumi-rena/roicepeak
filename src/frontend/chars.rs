use super::{bounded_read, u32_at};
use crate::{Result, invalid};
use std::path::Path;

pub(super) struct Characters {
    names: Vec<String>,
    entries: Vec<u32>,
    space: usize,
}

pub(super) struct Info {
    pub mask: u32,
    pub category: usize,
    pub length: usize,
    pub group: bool,
    pub invoke: bool,
}

impl Characters {
    pub fn open(path: &Path) -> Result<Self> {
        let bytes = bounded_read(path, 4 + 32 * 18 + 65_535 * 4)?;
        if bytes.len() < 4 {
            return Err(invalid("truncated character table"));
        }
        let count = u32_at(&bytes, 0) as usize;
        if count == 0 || count > 18 || bytes.len() != 4 + 32 * count + 65_535 * 4 {
            return Err(invalid("invalid character table dimensions"));
        }
        let mut names = Vec::new();
        for index in 0..count {
            let field = &bytes[4 + 32 * index..4 + 32 * (index + 1)];
            let length = field
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| invalid("unterminated character category name"))?;
            let name = std::str::from_utf8(&field[..length])
                .map_err(|_| invalid("character category is not UTF-8"))?;
            if name.is_empty() || names.iter().any(|old| old == name) {
                return Err(invalid("empty or duplicate character category"));
            }
            names.push(name.to_owned());
        }
        let space = names
            .iter()
            .position(|s| s == "SPACE")
            .ok_or_else(|| invalid("character table lacks SPACE category"))?;
        let mut entries = Vec::with_capacity(65_535);
        for at in (4 + 32 * count..bytes.len()).step_by(4) {
            let value = u32_at(&bytes, at);
            let category = ((value >> 18) & 255) as usize;
            let mask = value & 0x3ffff;
            if category >= count || mask == 0 || mask >> count != 0 || mask & (1 << category) == 0 {
                return Err(invalid("character category index or mask outside table"));
            }
            entries.push(value);
        }
        Ok(Self {
            names,
            entries,
            space,
        })
    }

    pub fn names(&self) -> &[String] {
        &self.names
    }

    pub fn info(&self, ch: char) -> Info {
        let value = self
            .entries
            .get(ch as usize)
            .copied()
            .unwrap_or(self.entries[0]);
        Info {
            mask: value & 0x3ffff,
            category: ((value >> 18) & 255) as usize,
            length: ((value >> 26) & 15) as usize,
            group: value & (1 << 30) != 0,
            invoke: value & (1 << 31) != 0,
        }
    }

    pub fn is_space(&self, ch: char) -> bool {
        self.info(ch).category == self.space
    }
}
