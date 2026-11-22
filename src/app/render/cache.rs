//! Bounded resource snapshots and worker-local compiled model caches.
#[path = "cache_memory.rs"]
pub(super) mod memory;
use super::{Request, VoiceBinding};
use crate::app::{
    dictionary::Entry,
    document::{Parameters, Segment, WordState},
};
use roicepeak::{
    Error, Result as CoreResult,
    frontend::Tagger,
    manifest::Manifest,
    speech_frontend::Language,
    synthesis::{Edits, ModelMemory},
};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
const MAX_MODEL_BYTES: u64 = 768 * 1024 * 1024;
const MAX_COMPILED_BYTES: usize = 768 * 1024 * 1024;
const MAX_COMPILED_VOICES: usize = 64;
const MAX_BASELINES: usize = 64;
const MAX_BASELINE_BYTES: usize = 64 * 1024 * 1024;
pub(super) fn read(path: &Path, limit: usize) -> CoreResult<Vec<u8>> {
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > limit as u64 {
        return Err(Error(
            "Synthesis resource must be a bounded regular file".into(),
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Error("Synthesis resource grew beyond byte limit".into()));
    }
    Ok(bytes)
}
#[derive(Clone, PartialEq)]
pub(super) struct Stamp {
    pub(super) path: PathBuf,
    pub(super) len: u64,
    pub(super) modified: SystemTime,
}
fn stamps(paths: &[PathBuf], limit: u64) -> CoreResult<Vec<Stamp>> {
    let mut total = 0u64;
    let mut result = Vec::new();
    for path in paths {
        let path = fs::canonicalize(path)?;
        let metadata = fs::metadata(&path)?;
        total = total
            .checked_add(metadata.len())
            .ok_or_else(|| Error("Resource size overflow".into()))?;
        if !metadata.is_file() || total > limit {
            return Err(Error("Synthesis resource-set byte limit".into()));
        }
        result.push(Stamp {
            path,
            len: metadata.len(),
            modified: metadata.modified()?,
        });
    }
    Ok(result)
}
pub(super) struct CachedVoice {
    pub(super) binding: Arc<VoiceBinding>,
    pub(super) path: PathBuf,
    pub(super) stamp: Vec<Stamp>,
    pub(super) manifest: Manifest,
    model_files: usize,
    voice_charge: usize,
    model_charge: ModelMemory,
}
pub(super) struct VoiceLease {
    pub(super) voice: Arc<CachedVoice>,
    pub(super) cache_hit: bool,
    pub(super) evicted: usize,
    pub(super) retained_bytes: usize,
    pub(super) retained_entries: usize,
    pub(super) retained: bool,
}
pub(super) struct ValidatedVoice {
    pub(super) binding: Arc<VoiceBinding>,
    pub(super) path: PathBuf,
    paths: Vec<PathBuf>,
    pub(super) stamp: Vec<Stamp>,
    model_files: usize,
}
pub(super) struct Dictionary {
    pub(super) stamp: Vec<Stamp>,
    pub(super) tagger: Tagger,
}
#[derive(Clone, PartialEq)]
pub(super) struct UnitKey {
    pub(super) language: roicepeak::speech_frontend::Language,
    pub(super) value_policy: roicepeak::scalar_layers::ValuePolicy,
    pub(super) text: String,
    pub(super) parameters: Parameters,
    pub(super) project_parameters: Parameters,
    pub(super) narrator_parameters: Parameters,
    pub(super) scalar_layers: roicepeak::scalar_layers::ScalarLayers,
}
#[derive(Clone)]
pub(super) struct Baseline {
    pub(super) unit: UnitKey,
    pub(super) authored: Option<Arc<AuthoredControls>>,
    pub(super) waiting_for: Option<u64>,
    pub(super) dictionary: Arc<Vec<Entry>>,
    pub(super) voice: Option<Arc<VoiceBinding>>,
    pub(super) model_stamp: Vec<Stamp>,
    pub(super) dictionary_stamp: Vec<Stamp>,
    pub(super) weights: Vec<f32>,
    pub(super) pcm: Arc<Vec<f32>>, // Before editor volume.
    pub(super) segments: Vec<Segment>,
    pub(super) refs: Vec<[usize; 5]>,
    pub(super) nucleus: Vec<bool>,
    pub(super) codes: Vec<u32>,
    pub(super) timing: Vec<super::PhoneTiming>,
    pub(super) neutral_words: Vec<WordState>,
    pub(super) word_query: crate::app::document::state::WordQueryState,
    pub(super) word_positions: crate::app::sentence_edit::WordPositions,
}
#[derive(PartialEq)]
pub(super) struct AuthoredControls {
    pub(super) token_signature: Vec<u8>,
    pub(super) edits: Edits,
}
impl Baseline {
    fn source(&self) -> Option<(&str, &str, &Path)> {
        self.voice.as_ref().map(|voice| {
            (
                self.unit.text.as_str(),
                self.unit.language.name(),
                voice.source.as_path(),
            )
        })
    }
}
#[derive(Default)]
pub(super) struct Cache {
    pub(super) dictionary: Option<Dictionary>,
    pub(super) voices: Vec<Arc<CachedVoice>>,
    pub(super) baseline: Vec<Baseline>,
    pub(super) queries: Vec<super::preparation::PreparedUnit>,
}
impl Cache {
    pub(super) fn reserve_audio(&mut self, request: &Request, generation: u64) {
        if self.baseline.is_empty() {
            return;
        }
        let existing: BTreeSet<_> = self.baseline.iter().filter_map(Baseline::source).collect();
        let mut pending = BTreeSet::new();
        'blocks: for block in &request.project.blocks {
            let Ok(voice) = request.config.binding(&block.narrator) else {
                continue;
            };
            for sentence in block.sentence_ranges() {
                let key = (
                    &block.text[sentence.bytes],
                    block.narrator.language.as_str(),
                    voice.source.as_path(),
                );
                if existing.contains(&key) && pending.insert(key) && pending.len() == existing.len()
                {
                    break 'blocks;
                }
            }
        }
        for entry in &mut self.baseline {
            if entry.source().is_some_and(|key| pending.contains(&key)) {
                entry.waiting_for = Some(generation);
            }
        }
    }

    pub(super) fn visit_audio(
        &mut self,
        text: &str,
        language: Language,
        voice: &Path,
        generation: u64,
    ) {
        let key = (text, language.name(), voice);
        for entry in &mut self.baseline {
            if entry.waiting_for == Some(generation) && entry.source() == Some(key) {
                entry.waiting_for = None;
            }
        }
    }

    pub(super) fn remember(&mut self, baseline: Baseline, generation: u64) {
        let bytes = baseline.estimated_bytes();
        if bytes > MAX_BASELINE_BYTES {
            return;
        }
        self.baseline.retain(|entry| {
            entry.unit != baseline.unit
                || entry.authored != baseline.authored
                || entry.dictionary != baseline.dictionary
                || entry.voice != baseline.voice
                || entry.model_stamp != baseline.model_stamp
                || entry.dictionary_stamp != baseline.dictionary_stamp
                || entry.weights != baseline.weights
        });
        let (mut total, reserved, reserved_count) = self.baseline.iter().fold(
            (0usize, 0usize, 0usize),
            |(total, reserved, count), entry| {
                let size = entry.estimated_bytes();
                let total = total.saturating_add(size);
                if entry.waiting_for == Some(generation) {
                    (total, reserved.saturating_add(size), count + 1)
                } else {
                    (total, reserved, count)
                }
            },
        );
        // Keep clips the current request has yet to visit.
        if reserved_count >= MAX_BASELINES || reserved.saturating_add(bytes) > MAX_BASELINE_BYTES {
            return;
        }
        while self.baseline.len() >= MAX_BASELINES
            || total.saturating_add(bytes) > MAX_BASELINE_BYTES
        {
            let Some(index) = self
                .baseline
                .iter()
                .position(|entry| entry.waiting_for != Some(generation))
            else {
                return;
            };
            total = total.saturating_sub(self.baseline.remove(index).estimated_bytes());
        }
        self.baseline.push(baseline);
    }

    pub(super) fn binding_snapshot(&self, binding: &VoiceBinding) -> Arc<VoiceBinding> {
        self.baseline
            .iter()
            .filter_map(|base| base.voice.as_ref())
            .chain(self.queries.iter().map(|entry| &entry.key.voice))
            .chain(self.voices.iter().map(|voice| &voice.binding))
            .find(|cached| cached.as_ref() == binding)
            .cloned()
            .unwrap_or_else(|| Arc::new(binding.clone()))
    }

    pub(super) fn dictionary(&mut self, path: &Path) -> CoreResult<()> {
        let paths = [
            "sys.dic",
            "unk.dic",
            "char.bin",
            "matrix.bin.400.usi",
            "matrix.bin.400.vhti",
        ]
        .iter()
        .map(|name| path.join(name))
        .collect::<Vec<_>>();
        let stamp = stamps(&paths, 256 * 1024 * 1024)?;
        if self.dictionary.as_ref().is_some_and(|d| d.stamp == stamp) {
            return Ok(());
        }
        let tagger = Tagger::open(path, &[])?;
        if stamps(&paths, 256 * 1024 * 1024)? != stamp {
            return Err(Error("Dictionary changed while loading".into()));
        }
        self.dictionary = Some(Dictionary { stamp, tagger });
        self.baseline.clear();
        self.queries.clear();
        Ok(())
    }
    pub(super) fn retained_bytes(&self) -> usize {
        let models =
            ModelMemory::retained_bytes(self.voices.iter().map(|voice| &voice.model_charge));
        self.voices.iter().fold(models, |total, voice| {
            total.saturating_add(voice.voice_charge)
        })
    }
    pub(super) fn validate_voice(&self, binding: &VoiceBinding) -> CoreResult<ValidatedVoice> {
        let path = fs::canonicalize(&binding.source)?;
        if read(&path, 8 * 1024 * 1024)?.as_slice() != binding.source_bytes.as_slice() {
            return Err(Error(
                "Voice metadata changed; reload the voice catalog".into(),
            ));
        }
        let resources = &binding.resources;
        let mut paths = resources.file_paths();
        let model_files = paths.len();
        paths.push(path.clone());
        for (path, expected) in &binding.metadata_files {
            if read(path, expected.len())?.as_slice() != expected.as_slice() {
                return Err(Error(
                    "Model metadata changed; reload the voice catalog".into(),
                ));
            }
            paths.push(path.clone());
        }
        let stamp = stamps(&paths, MAX_MODEL_BYTES)?;
        Ok(ValidatedVoice {
            binding: self.binding_snapshot(binding),
            path,
            paths,
            stamp,
            model_files,
        })
    }
    pub(super) fn voice(&mut self, validated: &ValidatedVoice) -> CoreResult<VoiceLease> {
        let ValidatedVoice {
            binding,
            path,
            paths,
            stamp,
            model_files,
        } = validated;
        if let Some(i) = self.voices.iter().position(|v| {
            &v.path == path && &v.stamp == stamp && v.binding.as_ref() == binding.as_ref()
        }) {
            let voice = self.voices.remove(i);
            self.voices.push(Arc::clone(&voice));
            return Ok(VoiceLease {
                voice,
                cache_hit: true,
                evicted: 0,
                retained_bytes: self.retained_bytes(),
                retained_entries: self.voices.len(),
                retained: true,
            });
        }
        let resources = &binding.resources;
        let shared = self.voices.iter().rev().find(|voice| {
            voice.model_files == *model_files
                && voice.stamp[..*model_files] == stamp[..*model_files]
                && voice.binding.resources.vocoder_effects == resources.vocoder_effects
        });
        let manifest = match shared {
            Some(voice) => Manifest::from_resources_reusing(resources, &voice.manifest)?,
            None => Manifest::from_resources(resources)?,
        };
        if &stamps(paths, MAX_MODEL_BYTES)? != stamp
            || read(&path, 8 * 1024 * 1024)?.as_slice() != binding.source_bytes.as_slice()
        {
            return Err(Error("Model resources changed while loading".into()));
        }
        let model_charge = shared.map_or_else(|| manifest.synth.model_memory(), |v| v.model_charge);
        let mut compiled = CachedVoice {
            binding: Arc::clone(binding),
            path: path.clone(),
            stamp: stamp.clone(),
            manifest,
            model_files: *model_files,
            voice_charge: 0,
            model_charge,
        };
        compiled.voice_charge = compiled.estimated_voice_bytes();
        let voice = Arc::new(compiled);
        let before = self.voices.len();
        self.voices
            .retain(|existing| existing.path != voice.path || existing.stamp == voice.stamp);
        let mut evicted = before - self.voices.len();
        let retained = voice
            .voice_charge
            .saturating_add(voice.model_charge.bytes())
            <= MAX_COMPILED_BYTES;
        if retained {
            self.voices.push(Arc::clone(&voice));
            while self.voices.len() > MAX_COMPILED_VOICES
                || self.retained_bytes() > MAX_COMPILED_BYTES
            {
                self.voices.remove(0);
                evicted += 1;
            }
        }
        self.queries.retain(|entry| entry.owner.strong_count() > 0);
        Ok(VoiceLease {
            voice,
            cache_hit: false,
            evicted,
            retained_bytes: self.retained_bytes(),
            retained_entries: self.voices.len(),
            retained,
        })
    }
}
