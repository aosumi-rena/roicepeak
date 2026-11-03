//! Bounded resolution inside one owner-provided plain resource root.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::{
    cell::RefCell,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

pub(super) struct Root {
    pub(super) directory: PathBuf,
    problems: RefCell<Vec<String>>,
}
impl Root {
    pub(super) fn plain(directory: PathBuf) -> Self {
        Self {
            directory,
            problems: RefCell::new(Vec::new()),
        }
    }
    pub(super) fn problem(&self, message: String) {
        self.problems.borrow_mut().push(message);
    }
    pub(super) fn take_problems(&self) -> Vec<String> {
        self.problems.take()
    }
}
impl AsRef<Path> for Root {
    fn as_ref(&self) -> &Path {
        &self.directory
    }
}
impl std::ops::Deref for Root {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.directory
    }
}

pub(super) fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(format!(
            "{} exceeds its resource byte limit",
            path.display()
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("Resource grew beyond its byte limit".into());
    }
    Ok(bytes)
}

pub(super) fn decode(value: &str, limit: usize) -> Result<Vec<u8>, String> {
    if value.len() > limit.saturating_add(2) / 3 * 4 + 4 {
        return Err("Inline voice resource byte limit".into());
    }
    let bytes = STANDARD
        .decode(value)
        .map_err(|e| format!("Invalid inline voice resource: {e}"))?;
    if bytes.len() > limit {
        return Err("Decoded voice resource byte limit".into());
    }
    Ok(bytes)
}

pub(super) fn id(value: &str) -> Result<&str, String> {
    if value.len() != 16 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Model component identity must be sixteen hexadecimal digits".into());
    }
    Ok(value)
}

pub(super) fn relative(relative: &Path) -> Result<(), String> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Voice resource must be a relative path inside its root".into());
    }
    Ok(())
}

pub(super) fn file(root: &Root, relative: &Path) -> Result<PathBuf, String> {
    self::relative(relative)?;
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let path = fs::canonicalize(root.join(relative))
        .map_err(|e| format!("{}: {e}", root.join(relative).display()))?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err("Voice resource escapes its root or is not a regular file".into());
    }
    Ok(path)
}

pub(super) fn flag(root: &Root, relative: &Path) -> Result<Option<(PathBuf, Vec<u8>)>, String> {
    if !root
        .join(relative)
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        return Ok(None);
    }
    let path = file(root, relative)?;
    let bytes = read(&path, 1)?;
    if !matches!(bytes.as_slice(), [0] | [1]) {
        return Err("Model flag must contain one zero/one byte".into());
    }
    Ok(Some((path, bytes)))
}
