//! Declared public portrait artwork, resolved only when its expression is selected.
use super::{Portrait, resolve};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};
#[derive(Clone, Debug)]
pub struct PortraitSources {
    directory: PathBuf,
    metadata: Arc<Vec<u8>>,
    declarations: BTreeMap<String, String>,
    default_region: Option<[i32; 4]>,
    identity: u64,
}
impl PortraitSources {
    pub(super) fn new(
        root: &resolve::Root,
        metadata: Arc<Vec<u8>>,
        declarations: BTreeMap<String, String>,
        default_region: Option<[i32; 4]>,
    ) -> Self {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        root.directory.hash(&mut hash);
        metadata.hash(&mut hash);
        Self {
            directory: root.directory.clone(),
            metadata,
            declarations,
            default_region,
            identity: hash.finish(),
        }
    }
    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }
    pub(crate) fn load(&self, engine_key: &str) -> Result<Option<Portrait>, String> {
        let Some(key) = self.declarations.get(engine_key) else {
            return Ok(None);
        };
        #[derive(Deserialize)]
        struct Description {
            entrance: Entrance,
        }
        #[derive(Deserialize)]
        struct Entrance {
            files: BTreeMap<String, String>,
            #[serde(default)]
            assets: BTreeMap<String, String>,
        }
        // The bounded metadata snapshot is already shared with the binding.
        // Do not retain duplicate encoded payloads for every emotion.
        let description: Description =
            serde_json::from_slice(&self.metadata).map_err(|error| error.to_string())?;
        let root = resolve::Root::plain(self.directory.clone());
        load_declared(
            &root,
            &description.entrance.files,
            &description.entrance.assets,
            key,
            16 * 1024 * 1024,
        )?
        .map(|bytes| portrait(bytes, self.default_region))
        .transpose()
    }
}
pub(super) fn load_declared(
    root: &resolve::Root,
    files: &BTreeMap<String, String>,
    assets: &BTreeMap<String, String>,
    key: &str,
    limit: usize,
) -> Result<Option<Vec<u8>>, String> {
    let bytes = if let Some(bytes) = files
        .get(key)
        .map(|value| resolve::decode(value, limit))
        .transpose()?
    {
        bytes
    } else {
        let Some(name) = assets.get(key) else {
            return Ok(None);
        };
        let asset = Path::new(name);
        let hash = asset
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or("Artwork asset lacks a content identity")?;
        let supported = asset.extension().is_some_and(|value| {
            value.eq_ignore_ascii_case("jpg")
                || value.eq_ignore_ascii_case("jpeg")
                || value.eq_ignore_ascii_case("png")
        });
        if !supported
            || asset.components().count() != 1
            || hash.len() != 64
            || !hash.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(
                "Artwork asset must declare a SHA-256 filename with a supported image extension"
                    .into(),
            );
        }
        let relative = PathBuf::from("assets").join(name);
        let path = resolve::file(root, &relative)?;
        let bytes = resolve::read(&path, limit)?;
        use sha2::{Digest, Sha256};
        if !crate::app::hex::encode(&Sha256::digest(&bytes)).eq_ignore_ascii_case(hash) {
            return Err("Artwork differs from its declared SHA-256 filename".into());
        }
        bytes
    };
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let (width, height) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 40_000_000 {
        return Err("Artwork pixel size limit".into());
    }
    Ok(Some(bytes))
}
pub(super) fn portrait(
    bytes: Vec<u8>,
    default_region: Option<[i32; 4]>,
) -> Result<Portrait, String> {
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let (width, height) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 40_000_000 {
        return Err("Portrait pixel size limit".into());
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    Ok(Portrait {
        content_key: hash.finish(),
        bytes: Arc::from(bytes),
        pixel_size: [width, height],
        default_region,
    })
}
