//! Bounded discovery under paths explicitly configured by the owner.
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const MAX_DEPTH: usize = 3;
const MAX_DIRECTORIES: usize = 1024;
const MAX_ENTRIES: usize = 8192;

pub(super) struct Source {
    pub(super) root: PathBuf,
    pub(super) voice: Option<PathBuf>,
}

#[derive(Default)]
pub(super) struct Discovery {
    pub(super) candidates: Vec<Source>,
    pub(super) problems: Vec<String>,
    directories: BTreeSet<PathBuf>,
    files: BTreeSet<PathBuf>,
    entries: usize,
}

pub(super) fn discover(paths: &[PathBuf]) -> Result<Discovery, String> {
    if paths.len() > 32 {
        return Err("Voice resource search path limit".into());
    }
    let mut found = Discovery::default();
    for configured in paths {
        let path = match fs::canonicalize(configured) {
            Ok(path) => path,
            Err(error) => {
                found
                    .problems
                    .push(format!("{}: {error}", configured.display()));
                continue;
            }
        };
        if path.is_dir() {
            // A voices/ path uses the adjacent networks/ and components/ directories.
            if path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("voices"))
            {
                if let Some(parent) = path.parent() {
                    found.candidates.push(Source {
                        root: parent.to_owned(),
                        voice: None,
                    });
                    continue;
                }
            }
            found.directory(&path, &path, 0)?;
        } else {
            found.file(&path, true);
        }
    }
    Ok(found)
}

impl Discovery {
    fn file(&mut self, path: &Path, explicit: bool) {
        if !self.files.insert(path.to_owned()) {
            return;
        }
        let extension = path.extension();
        if extension.is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
            && path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name.eq_ignore_ascii_case("voices"))
        {
            let root = path.parent().and_then(Path::parent).unwrap();
            self.candidates.push(Source {
                root: root.to_owned(),
                voice: Some(path.to_owned()),
            });
        } else if explicit {
            self.problems.push(format!(
                "{} is not a plain voice description",
                path.display()
            ));
        }
    }

    fn directory(&mut self, path: &Path, boundary: &Path, depth: usize) -> Result<(), String> {
        if !self.directories.insert(path.to_owned()) {
            return Ok(());
        }
        if self.directories.len() > MAX_DIRECTORIES {
            return Err(
                "Voice search exceeds 1024 directories; select a narrower directory".into(),
            );
        }
        let voice_root = path.join("voices").is_dir();
        if voice_root {
            self.candidates.push(Source {
                root: path.to_owned(),
                voice: None,
            });
        }
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                self.problems.push(format!("{}: {error}", path.display()));
                return Ok(());
            }
        };
        // Preserve configured-path and filesystem order.
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.problems.push(format!("{}: {error}", path.display()));
                    continue;
                }
            };
            self.entries += 1;
            if self.entries > MAX_ENTRIES {
                return Err(
                    "Voice search exceeds 8192 entries; select a narrower directory".into(),
                );
            }
            let candidate = match fs::canonicalize(entry.path()) {
                Ok(candidate) => candidate,
                Err(error) => {
                    self.problems
                        .push(format!("{}: {error}", entry.path().display()));
                    continue;
                }
            };
            if !candidate.starts_with(boundary) {
                self.problems.push(format!(
                    "{} resolves outside its configured search directory",
                    entry.path().display()
                ));
                continue;
            }
            if candidate.is_dir() {
                // Inspect adjacent banks without descending into model or artwork trees.
                if voice_root
                    && candidate.file_name().is_some_and(|name| {
                        ["voices", "networks", "components", "assets"]
                            .iter()
                            .any(|skip| name.eq_ignore_ascii_case(skip))
                    })
                {
                    continue;
                }
                if depth < MAX_DEPTH {
                    self.directory(&candidate, boundary, depth + 1)?;
                } else {
                    self.problems.push(format!(
                        "{} exceeds the search depth limit; add it directly if it contains voices",
                        candidate.display()
                    ));
                }
            } else {
                self.file(&candidate, false);
            }
        }
        Ok(())
    }
}
