//! Plain voice resource catalog.
mod artwork;
mod identity;
mod metadata;
pub use artwork::PortraitSources;
pub use identity::{EmotionControl, EmotionMetadata, IdentityMetadata};
mod resolve;
mod sources;

use crate::app::document::{VoiceRef, VoiceSourceInfo, resolve_source};
use crate::app::render::VoiceBinding;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct Portrait {
    pub bytes: Arc<[u8]>,
    pub pixel_size: [u32; 2],
    /// Artwork content identity for presentation cache invalidation only.
    pub content_key: u64,
    /// Source-image crop rectangle. Height -1 is valid; initial scale uses width.
    pub default_region: Option<[i32; 4]>,
}

#[derive(Clone, Debug)]
pub struct Voice {
    /// Runtime resource instance key.
    pub key: String,
    pub name: String,
    pub collection_name: String,
    pub version: u32,
    pub vendor: String,
    pub languages: Vec<String>,
    pub emotions: Vec<EmotionMetadata>,
    /// Commercial-use metadata from idc.json.
    pub can_commercial: bool,
    pub color: [u8; 3],
    pub avatar: Option<Vec<u8>>,
    pub portrait_main: Option<Portrait>,
    pub portrait_sources: PortraitSources,
    pub binding: VoiceBinding,
}

impl VoiceSourceInfo for Voice {
    fn identity(&self) -> &str {
        &self.name
    }
    fn languages(&self) -> &[String] {
        &self.languages
    }
    fn version(&self) -> u32 {
        self.version
    }
}

#[derive(Default, Clone, Debug)]
pub struct Catalog {
    pub voices: Vec<Voice>,
    pub problems: Vec<String>,
    identities: BTreeMap<String, IdentityMetadata>,
}

#[derive(Default)]
struct Loader {
    catalog: Catalog,
    sources: BTreeSet<PathBuf>,
    identities: BTreeSet<String>,
    metadata_bytes: usize,
    artwork_bytes: usize,
}
impl Loader {
    fn voice(&mut self, root: &resolve::Root, path: PathBuf) -> Result<(), String> {
        let path = match fs::canonicalize(&path) {
            Ok(path) => path,
            Err(error) => {
                self.catalog
                    .problems
                    .push(format!("{}: {error}", path.display()));
                return Ok(());
            }
        };
        if !path.starts_with(&root.directory) {
            self.catalog.problems.push(format!(
                "{} resolves outside its voice resource root",
                path.display()
            ));
            return Ok(());
        }
        if !self.sources.insert(path.clone()) {
            return Ok(());
        }
        if self.sources.len() > 256 {
            return Err("Voice catalog count limit".into());
        }
        let bytes = match resolve::read(&path, 8 * 1024 * 1024) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.catalog
                    .problems
                    .push(format!("{}: {error}", path.display()));
                return Ok(());
            }
        };
        self.metadata_bytes = self
            .metadata_bytes
            .checked_add(bytes.len())
            .ok_or("Voice metadata size overflow")?;
        if self.metadata_bytes > 64 * 1024 * 1024 {
            return Err("Voice catalog metadata limit".into());
        }
        match metadata::load(root, &path, bytes) {
            Ok(mut voice) => {
                if self.identities.insert(voice.key.clone()) {
                    let artwork = voice.avatar.as_ref().map_or(0, Vec::len)
                        + voice
                            .portrait_main
                            .as_ref()
                            .map_or(0, |portrait| portrait.bytes.len());
                    if self.artwork_bytes.saturating_add(artwork) > 64 * 1024 * 1024 {
                        voice.avatar = None;
                        voice.portrait_main = None;
                        self.catalog.problems.push(format!(
                            "{}: optional artwork exceeds the catalog's 64 MiB artwork budget",
                            path.display()
                        ));
                    } else {
                        self.artwork_bytes += artwork;
                    }
                    self.catalog.voices.push(voice);
                } else {
                    self.catalog.problems.push(format!("{}: narrator identity {:?} is already supplied by an earlier configured source; keeping that source", path.display(), voice.key));
                }
            }
            Err(error) => self
                .catalog
                .problems
                .push(format!("{}: {error}", path.display())),
        }
        self.catalog.problems.extend(
            root.take_problems()
                .into_iter()
                .map(|problem| format!("{}: {problem}", path.display())),
        );
        Ok(())
    }

    fn plain(&mut self, root: PathBuf, voice: Option<PathBuf>) -> Result<(), String> {
        let directory = root.join("voices");
        let mut entries = voice.into_iter().collect::<Vec<_>>();
        if entries.is_empty() {
            for entry in
                fs::read_dir(&directory).map_err(|e| format!("{}: {e}", directory.display()))?
            {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                {
                    entries.push(entry.path());
                    if entries.len() > 256 {
                        return Err("Voice catalog count limit".into());
                    }
                }
            }
        }
        if entries.is_empty() {
            self.catalog.problems.push(format!(
                "{} contains no voice descriptions",
                directory.display()
            ));
        }
        let root = resolve::Root::plain(root);
        for path in entries {
            self.voice(&root, path)?;
        }
        Ok(())
    }
}

impl Catalog {
    /// Discover plain descriptions only under explicitly configured paths.
    pub fn load_roots(roots: &[PathBuf]) -> Result<Self, String> {
        let discovered = sources::discover(roots)?;
        let mut loader = Loader::default();
        loader.catalog.problems = discovered.problems;
        for source in discovered.candidates {
            loader.plain(source.root, source.voice)?;
        }
        if !roots.is_empty() && loader.catalog.voices.is_empty() {
            loader.catalog.problems.push(
                "Configured search paths contain no usable complete plain voice descriptions"
                    .into(),
            );
        }
        loader.catalog.rebuild_identities();
        Ok(loader.catalog)
    }

    pub fn instance(&self, key: &str) -> Option<&Voice> {
        self.voices.iter().find(|voice| voice.key == key)
    }

    pub fn voice(&self, reference: &VoiceRef) -> Option<&Voice> {
        resolve_source(reference, &self.voices)
    }

    pub fn versions(&self, reference: &VoiceRef) -> Vec<u32> {
        let mut versions: Vec<_> = self
            .voices
            .iter()
            .filter(|voice| {
                voice.name == reference.identity && voice.languages.contains(&reference.language)
            })
            .map(|voice| voice.version)
            .collect();
        versions.sort_unstable_by(|a, b| b.cmp(a));
        versions
    }
}
