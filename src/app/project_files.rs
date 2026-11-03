//! Explicit project formats and staged native file replacement.
use crate::app::{
    document::Project, native_interop, native_project::NativeProject, render::Config,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static SERIAL: AtomicU64 = AtomicU64::new(0);
pub(crate) fn native(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("vpp"))
}
pub(crate) fn read(path: &Path, config: Option<&Config>) -> Result<Project, String> {
    if native(path) {
        native_interop::decode(NativeProject::read(path)?, config)
    } else {
        Project::read(path)
    }
}
pub(crate) fn write(path: &Path, project: &Project, config: Option<&Config>) -> Result<(), String> {
    if !native(path) {
        return project.write(path);
    }
    let bytes = native_interop::encode(project, config)?;
    let name = path
        .file_name()
        .ok_or("Project destination has no filename")?;
    let mut temporary = name.to_os_string();
    temporary.push(format!(
        ".roicepeak-{}-{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let temporary: PathBuf = path.with_file_name(temporary);
    let mut created = false;
    let result = (|| -> Result<(), String> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        created = true;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temporary);
    }
    result
}
