//! Snapshot-based export with independent block synthesis and cancellable file work.
#[path = "export_audio.rs"]
mod audio;
#[path = "export_dialog.rs"]
mod dialog;
#[path = "export_files.rs"]
mod files;
use crate::app::{document::Project, render};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    #[default]
    Wav,
    Flac,
}
impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => ".wav",
            Self::Flac => ".flac",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encoding {
    #[default]
    Utf8,
    ShiftJis,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NamePart {
    #[default]
    None,
    Number,
    Voice,
    Script,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub name: String,
    pub format: Format,
    pub rate: u32,
    pub scripts: bool,
    pub encoding: Encoding,
    pub text: bool,
    pub srt: bool,
    pub lab: bool,
    pub split: bool,
    pub parts: [NamePart; 3],
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            name: "Untitled".into(),
            format: Format::Wav,
            rate: 48000,
            scripts: false,
            encoding: Encoding::Utf8,
            text: true,
            srt: false,
            lab: false,
            split: false,
            parts: [NamePart::Number, NamePart::None, NamePart::None],
        }
    }
}
impl Settings {
    pub(crate) fn validate_stored(&self) -> Result<(), String> {
        if self.name.len() > 4096
            || self.name.contains('\0')
            || ![44100, 48000, 96000].contains(&self.rate)
        {
            return Err("Invalid stored export settings".into());
        }
        Ok(())
    }
    fn validate(&self, scope: &Scope) -> Result<(), String> {
        self.validate_stored()?;
        if self.name.is_empty() || self.name.len() > 4096 || self.name.contains('\0') {
            return Err("Enter a Save as name of at most 4096 bytes".into());
        }
        if ![44100, 48000, 96000].contains(&self.rate) {
            return Err("Choose a supported sample rate".into());
        }
        if matches!(scope, Scope::Project) && self.split && !self.parts.contains(&NamePart::Number)
        {
            return Err("Select 0, 1, 2 ... in the naming rule".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "blocks")]
pub enum Scope {
    Project,
    Blocks(Vec<u64>),
}
impl Default for Scope {
    fn default() -> Self {
        Self::Project
    }
}

enum Event {
    Accepted(Settings),
    PreparedBasis(Arc<render::ProgressBasis>),
    Progress(String, f64),
    Overwrite(Vec<PathBuf>, mpsc::Sender<bool>),
    Finished(Result<Vec<PathBuf>, String>),
}
struct Running {
    cancel: Arc<AtomicBool>,
    events: mpsc::Receiver<Event>,
}
#[derive(Default)]
pub struct State {
    pub settings: Settings,
    pub open: bool,
    scope: Scope,
    running: Option<Running>,
    overwrite: Option<(Vec<PathBuf>, mpsc::Sender<bool>)>,
    status: String,
    fraction: f64,
    rendering: bool,
    error: Option<String>,
    completed: Option<Vec<PathBuf>>,
    accepted: Option<Settings>,
    prepared: Option<Arc<render::ProgressBasis>>,
}
impl State {
    pub(crate) fn remember_preparation(
        &mut self,
        prepared: render::PreparedProject,
    ) -> Result<(), String> {
        self.prepared = Some(render::ProgressBasis::new(prepared)?);
        Ok(())
    }
    pub(crate) fn forget_preparation(&mut self) {
        self.prepared = None;
    }
    pub fn open(&mut self, scope: Scope, settings: &Settings) {
        if self.running.is_none() {
            self.settings = settings.clone();
            self.scope = scope;
            self.open = true;
            self.error = None;
            self.completed = None;
            self.accepted = None;
            self.fraction = 0.0;
            self.rendering = false;
        }
    }
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }
    pub fn active(&self) -> bool {
        self.open || self.running.is_some() || self.overwrite.is_some() || self.error.is_some()
    }
    pub fn take_completed(&mut self) -> Option<Vec<PathBuf>> {
        self.completed.take()
    }
    pub fn take_accepted(&mut self) -> Option<Settings> {
        self.accepted.take()
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        config: Option<&render::Config>,
        user_dictionary: Result<crate::app::dictionary::Snapshot, String>,
        names: &BTreeMap<String, String>,
        locale: crate::app::i18n::Locale,
    ) {
        if let Some(running) = &self.running {
            let mut finished = false;
            while let Ok(event) = running.events.try_recv() {
                match event {
                    Event::Accepted(settings) => {
                        self.accepted = Some(settings);
                        self.rendering = true;
                        self.open = false;
                    }
                    Event::PreparedBasis(basis) => self.prepared = Some(basis),
                    Event::Progress(status, fraction) => {
                        self.status = status;
                        self.fraction = fraction.clamp(0.0, 1.0);
                        self.open = false;
                    }
                    Event::Overwrite(paths, reply) => self.overwrite = Some((paths, reply)),
                    Event::Finished(result) => {
                        match result {
                            Ok(paths) => self.completed = Some(paths),
                            Err(error) if error == "Export cancelled" => {}
                            Err(error) => self.error = Some(error),
                        }
                        if self.completed.is_some() {
                            self.open = false;
                        }
                        finished = true;
                    }
                }
            }
            if finished {
                self.running = None;
                self.overwrite = None;
            }
        }
        if self.open && !self.rendering {
            if dialog::settings(self, ctx, project, names, locale) {
                match (config.cloned(), user_dictionary) {
                    (Some(config), Ok(dictionary)) => self.start(
                        ctx.clone(),
                        project.clone(),
                        config,
                        dictionary,
                        names.clone(),
                    ),
                    (_, Err(error)) => self.error = Some(error),
                    (None, Ok(_)) => {
                        self.error = Some(
                            "No voices are available. Select a voice resource folder in settings."
                                .into(),
                        )
                    }
                }
            }
        }
        if self.running.is_some() && self.rendering {
            dialog::progress(self, ctx, locale);
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        if !self.open {
            dialog::result(self, ctx, locale);
        }
    }
    fn start(
        &mut self,
        ctx: egui::Context,
        project: Project,
        config: render::Config,
        user_dictionary: crate::app::dictionary::Snapshot,
        names: BTreeMap<String, String>,
    ) {
        if let Err(error) = self.settings.validate(&self.scope) {
            self.error = Some(error);
            return;
        }
        let settings = self.settings.clone();
        let scope = self.scope.clone();
        let prepared = self.prepared.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let job_cancel = Arc::clone(&cancel);
        let (sender, events) = mpsc::channel();
        self.status = "Select the destination folder to save…".into();
        self.fraction = 0.0;
        self.rendering = false;
        let result = std::thread::Builder::new()
            .name("roicepeak-export".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    execute(
                        &project,
                        config,
                        user_dictionary,
                        &scope,
                        &settings,
                        &names,
                        &job_cancel,
                        &sender,
                        &ctx,
                        prepared,
                    )
                }))
                .unwrap_or_else(|_| Err("Export worker failed".into()));
                let _ = sender.send(Event::Finished(result));
                ctx.request_repaint();
            });
        match result {
            Ok(_) => self.running = Some(Running { cancel, events }),
            Err(error) => self.error = Some(format!("Could not start export: {error}")),
        }
    }
    fn cancel(&mut self) {
        if let Some(running) = &self.running {
            running.cancel.store(true, Ordering::Release);
        }
        if let Some((_, reply)) = self.overwrite.take() {
            let _ = reply.send(false);
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn check(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Export cancelled".into())
    } else {
        Ok(())
    }
}
fn progress(sender: &mpsc::Sender<Event>, ctx: &egui::Context, message: String, fraction: f64) {
    let _ = sender.send(Event::Progress(message, fraction));
    ctx.request_repaint();
}
fn execute(
    project: &Project,
    config: render::Config,
    user_dictionary: crate::app::dictionary::Snapshot,
    scope: &Scope,
    settings: &Settings,
    names: &BTreeMap<String, String>,
    cancel: &AtomicBool,
    sender: &mpsc::Sender<Event>,
    ctx: &egui::Context,
    prepared: Option<Arc<render::ProgressBasis>>,
) -> Result<Vec<PathBuf>, String> {
    let Some(directory) = rfd::FileDialog::new()
        .set_title("Please select the destination folder to save…")
        .pick_folder()
    else {
        return Err("Export cancelled".into());
    };
    execute_at(
        project,
        config,
        user_dictionary,
        scope,
        settings,
        names,
        &directory,
        cancel,
        sender,
        ctx,
        None,
        prepared,
    )
}

/// The same export backend without a GUI chooser, for an explicitly selected directory.
pub fn write_to_directory(
    request: render::Request,
    scope: Scope,
    settings: Settings,
    names: BTreeMap<String, String>,
    directory: &Path,
    overwrite: bool,
) -> Result<Vec<PathBuf>, String> {
    let (sender, _events) = mpsc::channel();
    execute_at(
        &request.project,
        request.config,
        request.user_dictionary,
        &scope,
        &settings,
        &names,
        directory,
        &AtomicBool::new(false),
        &sender,
        &egui::Context::default(),
        Some(overwrite),
        None,
    )
}

fn execute_at(
    project: &Project,
    config: render::Config,
    user_dictionary: crate::app::dictionary::Snapshot,
    scope: &Scope,
    settings: &Settings,
    names: &BTreeMap<String, String>,
    directory: &Path,
    cancel: &AtomicBool,
    sender: &mpsc::Sender<Event>,
    ctx: &egui::Context,
    approved_overwrite: Option<bool>,
    prepared: Option<Arc<render::ProgressBasis>>,
) -> Result<Vec<PathBuf>, String> {
    check(cancel)?;
    if !directory.is_dir() {
        return Err("Choose an existing export directory".into());
    }
    let outputs = files::plan(project, scope, settings, names, directory)?;
    let collisions: Vec<_> = outputs
        .iter()
        .flat_map(|output| output.paths.iter())
        .filter(|path| path.exists())
        .cloned()
        .collect();
    let overwrite: BTreeSet<_> = collisions.iter().cloned().collect();
    if !collisions.is_empty() {
        if approved_overwrite == Some(false) {
            return Err("Export destination already exists".into());
        }
        if approved_overwrite.is_none() {
            let (reply, response) = mpsc::channel();
            sender
                .send(Event::Overwrite(collisions, reply))
                .map_err(|_| "Export window closed")?;
            ctx.request_repaint();
            loop {
                check(cancel)?;
                match response.recv_timeout(Duration::from_millis(30)) {
                    Ok(true) => break,
                    Ok(false) => return Err("Export cancelled".into()),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => return Err("Export window closed".into()),
                }
            }
        }
    }
    check(cancel)?;
    let _ = sender.send(Event::Accepted(settings.clone()));
    ctx.request_repaint();
    let mut worker = render::Worker::default();
    let nonempty = project
        .blocks
        .iter()
        .any(|block| !block.text.trim().is_empty());
    let basis = if !nonempty {
        None
    } else if let Some(basis) =
        prepared.filter(|basis| basis.matches(project, &config, &user_dictionary))
    {
        Some(basis)
    } else {
        progress(sender, ctx, "Preparing export timing…".into(), 0.0);
        worker.prepare(render::Request {
            config: config.clone(),
            project: project.clone(),
            user_dictionary: user_dictionary.clone(),
        })?;
        Some(loop {
            if let Err(error) = check(cancel) {
                worker.cancel();
                return Err(error);
            }
            if let Some(prepared) = worker.poll_prepare() {
                break render::ProgressBasis::new(prepared?)?;
            }
            std::thread::sleep(Duration::from_millis(15));
        })
    };
    if let Some(basis) = &basis {
        let _ = sender.send(Event::PreparedBasis(Arc::clone(basis)));
    }
    let weights = outputs
        .iter()
        .map(|output| {
            basis
                .as_ref()
                .map_or(Ok(0), |basis| basis.samples(output.block))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let total = weights
        .iter()
        .try_fold(0usize, |sum, weight| sum.checked_add(*weight))
        .ok_or("Export progress sample total overflow")?;
    let fraction = |samples: usize| {
        if total == 0 {
            0.0
        } else {
            samples as f64 / total as f64
        }
    };
    let mut completed = 0usize;
    let mut determinate = true;
    let mut last_fraction = 0.0;
    let mut written = Vec::new();
    for (index, output) in outputs.iter().enumerate() {
        check(cancel)?;
        let message = format!(
            "Exporting {} / {}: {}",
            index + 1,
            outputs.len(),
            output.name
        );
        if determinate {
            last_fraction = fraction(completed);
        }
        progress(sender, ctx, message.clone(), last_fraction);
        let rendered =
            if let Some(basis) = basis.as_ref().filter(|basis| !basis.empty(output.block)) {
                let observer = worker.submit_with_progress(basis.plan(output.block)?)?;
                let mut previous = 0usize;
                Some(loop {
                    if let Err(error) = check(cancel) {
                        worker.cancel();
                        return Err(error);
                    }
                    if observer.invalidated() && determinate {
                        determinate = false;
                        progress(
                            sender,
                            ctx,
                            "Rendering continues; prepared progress is no longer current".into(),
                            last_fraction,
                        );
                    }
                    if let Some(snapshot) = observer.snapshot().filter(|_| determinate) {
                        if snapshot.total_samples != weights[index] {
                            determinate = false;
                            progress(
                                sender,
                                ctx,
                                "Rendering continues; progress total changed".into(),
                                last_fraction,
                            );
                        } else if snapshot.ready_samples != previous {
                            previous = snapshot.ready_samples;
                            last_fraction = fraction(completed + previous);
                            progress(sender, ctx, message.clone(), last_fraction);
                        }
                    }
                    if let Some(rendered) = worker.poll() {
                        let rendered = rendered?;
                        // Publication follows the reporter's final validity check.
                        if observer.invalidated() && determinate {
                            determinate = false;
                            progress(
                                sender,
                                ctx,
                                "Rendering completed; prepared progress was invalidated".into(),
                                last_fraction,
                            );
                        }
                        break rendered;
                    }
                    std::thread::sleep(Duration::from_millis(15));
                })
            } else {
                None
            };
        let blocks = rendered
            .as_ref()
            .map(|rendered| rendered.blocks.as_slice())
            .unwrap_or(&[]);
        let pcm = rendered
            .as_ref()
            .map(|rendered| rendered.pcm.as_slice())
            .unwrap_or(&[]);
        let audio = audio::encode(settings.format, pcm, settings.rate)?;
        let mut payloads = vec![(output.paths[0].clone(), audio)];
        if settings.scripts {
            let mut next = 1;
            if settings.text {
                payloads.push((
                    output.paths[next].clone(),
                    files::encode(&files::text(project, output.block), settings.encoding)?,
                ));
                next += 1;
            }
            if settings.srt {
                payloads.push((
                    output.paths[next].clone(),
                    files::encode(
                        &files::srt(
                            project,
                            blocks,
                            output.block,
                            output.block.and_then(|id| {
                                basis.as_ref().and_then(|basis| basis.block_start(id))
                            }),
                        ),
                        settings.encoding,
                    )?,
                ));
                next += 1;
            }
            if settings.lab {
                payloads.push((
                    output.paths[next].clone(),
                    files::lab(blocks, output.block, output.block.is_some()).into_bytes(),
                ));
            }
        }
        for (path, bytes) in payloads {
            check(cancel)?;
            write(&path, &bytes, overwrite.contains(&path))?;
            written.push(path);
        }
        completed += weights[index];
        if index + 1 == outputs.len() {
            last_fraction = 1.0;
        } else if determinate {
            last_fraction = fraction(completed);
        }
        progress(sender, ctx, String::new(), last_fraction);
    }
    Ok(written)
}
fn write(path: &Path, bytes: &[u8], overwrite: bool) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true);
    if overwrite {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("{}: {error}", path.display()))
}
