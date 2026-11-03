//! Unnormalized emotion-row mixtures and explicit model conditioning offsets.
use crate::{Result, dnni::Node, invalid, phonemes::Cursor};
use std::collections::BTreeMap;
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Offsets {
    pub aperiodicity: Option<usize>,
    pub volume: Option<usize>,
    pub intonation: Option<usize>,
    pub speed: Option<usize>,
    pub tension: Option<usize>,
}
pub struct Style {
    pub embedding: Vec<f32>,
    pub aperiodicity: Option<[f32; 5]>,
    pub volume_db: f32,
    pub intonation: f32,
    pub log_speed: f32,
    pub phoneme_log_speeds: BTreeMap<String, f32>,
    pub tension: f32,
}
impl Style {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            crate::memory::values(&self.embedding),
            crate::memory::btree(&self.phoneme_log_speeds, crate::memory::string, |_| 0),
        ])
    }

    pub fn gain(&self) -> f64 {
        10.0f64.powf(f64::from(self.volume_db) / 20.0)
    }
}
pub struct Stylebook {
    pub emotions: Vec<String>,
    base: Vec<f32>,
    rows: Vec<Vec<f32>>,
    offsets: Offsets,
    phonemes: Vec<String>,
}
impl Stylebook {
    /// Retained heap charge, excluding the inline Stylebook itself.
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        m::sum([
            m::nested(&self.emotions, m::string),
            m::values(&self.base),
            m::nested(&self.rows, m::values),
            m::nested(&self.phonemes, m::string),
        ])
    }

    pub fn compile(
        root: &Node<'_>,
        offsets: Option<Offsets>,
        phonemes: Vec<String>,
    ) -> Result<Self> {
        if !["_stbkv0", "_stbkv1"].contains(&root.tag.as_str()) {
            return Err(invalid("unsupported stylebook"));
        }
        let mut c = Cursor::new(root.payload);
        let emotions = c.strings()?;
        c.finish()?;
        let rows = if root.tag == "_stbkv0" {
            &root.children
        } else {
            &root
                .children
                .first()
                .ok_or_else(|| invalid("empty stylebook"))?
                .children
        };
        if rows.len() != emotions.len() + 1 || emotions.is_empty() || emotions.len() > 1024 {
            return Err(invalid("stylebook row count"));
        }
        let mut vectors = Vec::new();
        for row in &rows[1..] {
            if row.tag != "prim1" {
                return Err(invalid("stylebook row must be vector"));
            }
            vectors.push(row.tensor()?.values().to_vec());
        }
        let width = vectors[0].len();
        if vectors.iter().any(|v| v.len() != width) {
            return Err(invalid("stylebook row widths differ"));
        }
        let base = if rows[0].tag == "prim1" {
            rows[0].tensor()?.values().to_vec()
        } else if rows[0].tag == "cmpno" {
            vec![0.0; width]
        } else {
            return Err(invalid("unknown stylebook base"));
        };
        if base.len() != width {
            return Err(invalid("stylebook base width"));
        }
        let mut offsets = offsets.unwrap_or_default();
        for v in [
            &mut offsets.aperiodicity,
            &mut offsets.volume,
            &mut offsets.intonation,
            &mut offsets.speed,
            &mut offsets.tension,
        ] {
            if *v == Some(0) {
                *v = None;
            }
        }
        if offsets.aperiodicity.is_none()
            && offsets.volume.is_none()
            && offsets.intonation.is_none()
            && offsets.speed.is_none()
            && offsets.tension.is_none()
        {
            offsets.aperiodicity = (width > 32).then_some(32);
            offsets.volume = (width > 37).then_some(37);
            offsets.intonation = (width > 38).then_some(38);
        }
        let range = |start: Option<usize>, count: usize| {
            start.is_none_or(|s| s.checked_add(count).is_some_and(|end| end <= width))
        };
        if !range(offsets.aperiodicity, 5)
            || !range(offsets.volume, 1)
            || !range(offsets.intonation, 1)
            || !range(offsets.speed, 1 + phonemes.len())
            || !range(offsets.tension, 1)
        {
            return Err(invalid("stylebook offset outside row"));
        }
        Ok(Self {
            emotions,
            base,
            rows: vectors,
            offsets,
            phonemes,
        })
    }
    pub fn vector(&self, weights: &[f32]) -> Result<Vec<f32>> {
        if weights.len() != self.rows.len() || weights.iter().any(|v| !v.is_finite()) {
            return Err(invalid(
                "style weights must be finite and match emotion count",
            ));
        }
        let mut out = vec![0.0; self.base.len()];
        for (w, row) in weights.iter().zip(&self.rows) {
            for (dst, v) in out.iter_mut().zip(row) {
                *dst += w * v;
            }
        }
        for (dst, base) in out.iter_mut().zip(&self.base) {
            *dst += base;
        }
        if out.iter().any(|v| !v.is_finite()) {
            return Err(invalid("style mixture overflow"));
        }
        Ok(out)
    }
    pub fn mix(&self, weights: &[f32]) -> Result<Style> {
        let v = self.vector(weights)?;
        let o = &self.offsets;
        let scalar = |i: Option<usize>| i.map(|i| v[i]).unwrap_or(0.0);
        let width = o.aperiodicity.unwrap_or(v.len());
        Ok(Style {
            embedding: v[..width].to_vec(),
            aperiodicity: o.aperiodicity.map(|i| v[i..i + 5].try_into().unwrap()),
            volume_db: scalar(o.volume),
            intonation: scalar(o.intonation),
            log_speed: scalar(o.speed),
            tension: scalar(o.tension),
            phoneme_log_speeds: if let Some(start) = o.speed {
                self.phonemes
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (n.clone(), v[start + 1 + i]))
                    .collect()
            } else {
                BTreeMap::new()
            },
        })
    }
}
/// Speaker tables store channels by speaker, whereas inference receives one row.
pub fn speaker_vector(owner: &Node<'_>, index: usize) -> Result<Vec<f32>> {
    let n = if owner.tag == "modl4" {
        owner
    } else {
        owner
            .children
            .iter()
            .find(|c| c.tag == "modl4")
            .ok_or_else(|| invalid("owner has no speaker table"))?
    };
    if n.children.len() != 1 {
        return Err(invalid("speaker table structure"));
    }
    let t = n.children[0].tensor()?;
    if index >= t.cols() {
        return Err(invalid("speaker index outside table"));
    }
    Ok((0..t.rows()).map(|i| t.row(i)[index]).collect())
}

/// Raw custom vectors are consecutive slices for each owner in model order.
pub fn split_speakers(values: &[f32], owners: &[&Node<'_>]) -> Result<BTreeMap<String, Vec<f32>>> {
    if values.iter().any(|v| !v.is_finite()) {
        return Err(invalid("nonfinite custom speaker"));
    }
    let mut out = BTreeMap::new();
    let mut offset = 0usize;
    for owner in owners {
        let table = owner
            .children
            .iter()
            .find(|n| n.tag == "modl4")
            .ok_or_else(|| invalid("speaker owner has no table"))?;
        if table.children.len() != 1 {
            return Err(invalid("custom speaker table structure"));
        }
        let width = table.children[0].tensor()?.rows();
        let end = offset
            .checked_add(width)
            .ok_or_else(|| invalid("custom speaker size overflow"))?;
        let row = values
            .get(offset..end)
            .ok_or_else(|| invalid("custom speaker vector truncated"))?
            .to_vec();
        if out.insert(owner.tag.clone(), row).is_some() {
            return Err(invalid("duplicate custom speaker owner"));
        }
        offset = end;
    }
    if offset != values.len() {
        return Err(invalid("trailing custom speaker values"));
    }
    Ok(out)
}
