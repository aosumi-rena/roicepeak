use super::{
    Config, JobKind, MAX_AUDIO, PreparedProject, Request, SAMPLE_RATE, Shared, cache::Stamp,
};
use crate::app::{
    document::{OffsetMode, Project},
    placement,
    sentences::SentenceRef,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

struct Unit {
    block: u64,
    sentence: SentenceRef,
    duration: f64,
    samples: usize,
    model: Arc<[Stamp]>,
    dictionary: Arc<[Stamp]>,
}

pub struct ProgressBasis {
    source: Project,
    user_dictionary: crate::app::dictionary::Snapshot,
    config: Config,
    units: Vec<Unit>,
    groups: BTreeMap<u64, std::ops::Range<usize>>,
    source_index: BTreeMap<u64, usize>,
    starts: BTreeMap<u64, f64>,
    samples: usize,
}

impl ProgressBasis {
    pub fn new(prepared: PreparedProject) -> Result<Arc<Self>, String> {
        let mut groups = BTreeMap::new();
        let source_index: BTreeMap<_, _> = prepared
            .source
            .blocks
            .iter()
            .enumerate()
            .map(|(index, b)| (b.id, index))
            .collect();
        let ranges: Vec<_> = prepared
            .source
            .blocks
            .iter()
            .map(|b| b.sentence_ranges())
            .collect();
        let dictionary: Arc<[Stamp]> = prepared
            .units
            .first()
            .map_or_else(|| Arc::from([]), |u| Arc::from(u.dictionary_stamp.clone()));
        let mut units = Vec::with_capacity(prepared.units.len());
        let mut previous = None;
        let mut model: Arc<[Stamp]> = Arc::from([]);
        for unit in &prepared.units {
            let index = *source_index
                .get(&unit.block)
                .ok_or("Unknown prepared progress block")?;
            if ranges[index].get(unit.sentence.index) != Some(&unit.sentence)
                || unit.dictionary_stamp.as_slice() != dictionary.as_ref()
                || !unit.duration.is_finite()
                || unit.duration < 0.0
            {
                return Err("Invalid prepared progress sentence".into());
            }
            let samples = unit.duration * f64::from(SAMPLE_RATE);
            if !samples.is_finite() || samples > MAX_AUDIO as f64 {
                return Err("Prepared progress sentence exceeds audio limit".into());
            }
            if previous != Some(unit.block) {
                if groups.contains_key(&unit.block) {
                    return Err("Prepared progress block order is not contiguous".into());
                }
                groups.insert(unit.block, units.len()..units.len());
                model = Arc::from(unit.model_stamp.clone());
                previous = Some(unit.block);
            } else if unit.model_stamp.as_slice() != model.as_ref() {
                return Err("Prepared progress model changed within a block".into());
            }
            units.push(Unit {
                block: unit.block,
                sentence: unit.sentence.clone(),
                duration: unit.duration,
                samples: samples.round() as usize,
                model: Arc::clone(&model),
                dictionary: Arc::clone(&dictionary),
            });
            groups.get_mut(&unit.block).unwrap().end = units.len();
        }
        let basis = Arc::new(Self {
            source: prepared.source,
            user_dictionary: prepared.user_dictionary,
            config: prepared.config,
            units,
            groups,
            source_index,
            starts: prepared
                .blocks
                .iter()
                .map(|block| (block.id, block.start))
                .collect(),
            samples: prepared.samples,
        });
        if basis.geometry(None)?.1 != basis.samples {
            return Err("Prepared progress total disagrees with placement".into());
        }
        Ok(basis)
    }

    fn geometry(&self, selected: Option<u64>) -> Result<(Vec<(usize, usize)>, usize), String> {
        let mut planner = placement::Planner::new(MAX_AUDIO);
        let mut entries = Vec::new();
        let indices = match selected {
            Some(id) => {
                let index = *self
                    .source_index
                    .get(&id)
                    .ok_or("Unknown export progress block")?;
                index..index + 1
            }
            None => 0..self.source.blocks.len(),
        };
        for block in &self.source.blocks[indices] {
            let range = self.groups.get(&block.id).cloned().unwrap_or(0..0);
            let durations: Vec<_> = self.units[range.clone()]
                .iter()
                .map(|u| u.duration)
                .collect();
            let mut reset;
            let placed_block = if selected.is_some() {
                // Planner reads placement fields only; avoid cloning authored text/words.
                reset = crate::app::document::Block::new(block.id);
                reset.offset_mode = OffsetMode::Absolute;
                reset.offset_seconds = 0.0;
                reset.timestamp = 0.0;
                &reset
            } else {
                block
            };
            let placed = planner.append(placed_block, &durations)?;
            entries.extend(
                range
                    .zip(placed.sentences)
                    .map(|(index, p)| (index, p.sample_start)),
            );
        }
        Ok((entries, planner.sample_count()?))
    }

    pub fn samples(&self, selected: Option<u64>) -> Result<usize, String> {
        if selected.is_none() {
            Ok(self.samples)
        } else {
            self.geometry(selected).map(|v| v.1)
        }
    }

    pub fn matches(
        &self,
        source: &Project,
        config: &Config,
        user_dictionary: &crate::app::dictionary::Snapshot,
    ) -> bool {
        self.source.same_content(source)
            && self.config == *config
            && self.user_dictionary.same(user_dictionary)
    }

    pub fn block_start(&self, block: u64) -> Option<f64> {
        self.starts.get(&block).copied()
    }

    pub fn empty(&self, selected: Option<u64>) -> bool {
        match selected {
            Some(id) => self
                .source_index
                .get(&id)
                .is_some_and(|index| self.source.blocks[*index].text.trim().is_empty()),
            None => self.source.blocks.iter().all(|b| b.text.trim().is_empty()),
        }
    }

    pub fn plan(self: &Arc<Self>, selected: Option<u64>) -> Result<ProgressPlan, String> {
        let (entries, total) = self.geometry(selected)?;
        let mut source = self.source.clone();
        if let Some(id) = selected {
            source.blocks.retain(|b| b.id == id);
            for block in &mut source.blocks {
                block.offset_mode = OffsetMode::Absolute;
                block.offset_seconds = 0.0;
                block.timestamp = 0.0;
            }
        }
        let mut suffix = vec![total; entries.len() + 1];
        for index in (0..entries.len()).rev() {
            let (unit, start) = entries[index];
            suffix[index] = if self.units[unit].samples == 0 {
                suffix[index + 1]
            } else {
                suffix[index + 1].min(start.min(total))
            };
        }
        Ok(ProgressPlan {
            request: Request {
                config: self.config.clone(),
                project: source,
                user_dictionary: self.user_dictionary.clone(),
            },
            pending: Pending {
                basis: Arc::clone(self),
                entries,
                suffix,
                total,
                prefix: Arc::new(AtomicUsize::new(0)),
                valid: Arc::new(AtomicBool::new(true)),
            },
        })
    }
}

pub struct ProgressPlan {
    request: Request,
    pending: Pending,
}
impl ProgressPlan {
    pub(super) fn into_job(self) -> (Request, Pending) {
        (self.request, self.pending)
    }
}

pub(super) struct Pending {
    basis: Arc<ProgressBasis>,
    entries: Vec<(usize, usize)>,
    suffix: Vec<usize>,
    pub(super) total: usize,
    pub(super) prefix: Arc<AtomicUsize>,
    pub(super) valid: Arc<AtomicBool>,
}
impl Pending {
    pub(super) fn bind(self, generation: u64, shared: Weak<Shared>) -> Reporter {
        Reporter {
            pending: self,
            generation,
            shared,
            next: 0,
        }
    }
}

pub struct RenderProgress {
    pub(super) generation: u64,
    pub(super) shared: Weak<Shared>,
    pub(super) prefix: Arc<AtomicUsize>,
    pub(super) valid: Arc<AtomicBool>,
    pub(super) total: usize,
}
pub struct ProgressSnapshot {
    pub generation: u64,
    pub ready_samples: usize,
    pub total_samples: usize,
}
impl RenderProgress {
    pub fn invalidated(&self) -> bool {
        !self.valid.load(Ordering::Acquire)
    }
    pub fn snapshot(&self) -> Option<ProgressSnapshot> {
        let shared = self.shared.upgrade()?;
        if shared.generation.load(Ordering::Acquire) != self.generation || self.invalidated() {
            return None;
        }
        let ready_samples = self.prefix.load(Ordering::Acquire);
        if shared.generation.load(Ordering::Acquire) != self.generation || self.invalidated() {
            return None;
        }
        Some(ProgressSnapshot {
            generation: self.generation,
            ready_samples,
            total_samples: self.total,
        })
    }
}

pub(super) struct Reporter {
    pending: Pending,
    generation: u64,
    shared: Weak<Shared>,
    next: usize,
}
impl Reporter {
    pub(super) fn complete(
        &mut self,
        block: u64,
        sentence: &SentenceRef,
        samples: usize,
        pcm_samples: Option<usize>,
        model: &[Stamp],
        dictionary: &[Stamp],
    ) {
        if !self.pending.valid.load(Ordering::Acquire) {
            return;
        }
        let Some(&(index, _)) = self.pending.entries.get(self.next) else {
            self.pending.valid.store(false, Ordering::Release);
            return;
        };
        let expected = &self.pending.basis.units[index];
        if block != expected.block
            || sentence != &expected.sentence
            || samples != expected.samples
            || pcm_samples != Some(samples)
            || model != expected.model.as_ref()
            || dictionary != expected.dictionary.as_ref()
        {
            self.pending.valid.store(false, Ordering::Release);
            return;
        }
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        if super::checkpoint(&shared, self.generation, JobKind::Render).is_err() {
            return;
        }
        self.next += 1;
        self.pending
            .prefix
            .store(self.pending.suffix[self.next], Ordering::Release);
    }

    pub(super) fn finish(&self, samples: usize) {
        if self.next != self.pending.entries.len() || samples != self.pending.total {
            self.pending.valid.store(false, Ordering::Release);
        }
    }
}
