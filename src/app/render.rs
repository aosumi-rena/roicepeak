//! Independent background text synthesis from explicit owner-selected resources.
mod cache;
mod config;
mod edits;
mod engine;
mod playback_unit;
mod preparation;
mod profiling;
mod progress;
mod streaming;
mod unit;
mod worker;
use crate::app::document::{Project, Segment};
pub use config::{Config, EmotionBinding, VoiceBinding};
pub use profiling::{RenderProfile, diagnostics_path};
pub use progress::{ProgressBasis, ProgressPlan, RenderProgress};
use roicepeak::{Error, Result as CoreResult};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::SyncSender,
};
use std::time::Instant;
pub use streaming::Playback;
pub const SAMPLE_RATE: u32 = 48_000;
const MAX_AUDIO: usize = SAMPLE_RATE as usize * 60 * 10;
pub struct Request {
    pub config: Config,
    pub project: Project,
    pub user_dictionary: crate::app::dictionary::Snapshot,
}
pub struct Rendered {
    pub pcm: Arc<Vec<f32>>,
    pub rate: u32,
    pub blocks: Vec<RenderedBlock>,
    pub source: Project,
    pub(crate) user_dictionary: crate::app::dictionary::Snapshot,
    pub profile: RenderProfile,
}
/// Phone/timing metadata only. Preparation never supplies or implies PCM.
pub struct PreparedProject {
    pub(crate) config: Config,
    pub source: Project,
    pub(crate) user_dictionary: crate::app::dictionary::Snapshot,
    pub blocks: Vec<RenderedBlock>,
    pub units: Vec<PreparedSentence>,
    pub profile: RenderProfile,
    pub samples: usize,
}
/// Original sentence address and placement.
#[derive(Clone)]
pub struct PreparedSentence {
    pub block: u64,
    pub sentence: crate::app::sentences::SentenceRef,
    pub start: f64,
    pub duration: f64,
    pub sample_start: usize,
    /// Indices into the corresponding block's segments.
    pub segment_range: std::ops::Range<usize>,
    model_stamp: Vec<cache::Stamp>,
    dictionary_stamp: Vec<cache::Stamp>,
}
pub struct RenderedBlock {
    pub id: u64,
    pub start: f64,
    pub duration: f64,
    pub segments: Vec<Segment>,
    pub sentences: Vec<crate::app::document::SentenceState>,
    pub timing: Vec<SentenceTiming>,
}
/// Transient controlled query timing; it is never serialized into the project.
#[derive(Clone)]
pub struct PhoneTiming {
    pub reference: [usize; 5],
    pub symbol: String,
    pub start: f64,
    pub duration: f64,
}
pub struct SentenceTiming {
    pub sentence: crate::app::sentences::SentenceRef,
    pub start: f64,
    pub phones: Vec<PhoneTiming>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum JobKind {
    Render,
    Prepare,
    Playback,
}
impl JobKind {
    fn name(self) -> &'static str {
        match self {
            Self::Render => "render",
            Self::Prepare => "prepare",
            Self::Playback => "playback",
        }
    }
    fn current(self, shared: &Shared) -> &AtomicU64 {
        match self {
            Self::Render => &shared.generation,
            Self::Prepare => &shared.prepare_generation,
            Self::Playback => &shared.playback_generation,
        }
    }
    fn busy(self, shared: &Shared) -> &AtomicU64 {
        match self {
            Self::Render => &shared.busy,
            Self::Prepare => &shared.preparing,
            Self::Playback => &shared.playback_busy,
        }
    }
}
struct Job {
    generation: u64,
    kind: JobKind,
    request: Request,
    submitted: Instant,
    profile: RenderProfile,
    playback_start: f64,
    playback_block: Option<u64>,
    progress: Option<progress::Reporter>,
}
#[derive(Default)]
struct Pending {
    render: Option<Job>,
    prepare: Option<Job>,
    playback: Option<Job>,
}
impl Pending {
    fn take(&mut self) -> Option<Job> {
        self.render
            .take()
            .or_else(|| self.playback.take())
            .or_else(|| self.prepare.take())
    }
}
#[derive(Default)]
struct Shared {
    closed: AtomicBool,
    sequence: AtomicU64,
    generation: AtomicU64,
    prepare_generation: AtomicU64,
    playback_generation: AtomicU64,
    busy: AtomicU64,
    preparing: AtomicU64,
    playback_busy: AtomicU64,
    pending: Mutex<Pending>,
    result: Mutex<Option<(u64, Result<Rendered, String>)>>,
    prepare_result: Mutex<Option<(u64, Result<PreparedProject, String>)>>,
    playback_result: Mutex<Option<(u64, Result<Arc<Playback>, String>)>>,
    playback_session: Mutex<Option<std::sync::Weak<Playback>>>,
    diagnostics: Mutex<profiling::Ledger>,
}
pub struct Worker {
    shared: Arc<Shared>,
    wake: SyncSender<()>,
    start_error: Option<String>,
}
fn checkpoint(shared: &Shared, generation: u64, kind: JobKind) -> CoreResult<()> {
    if kind.current(shared).load(Ordering::Acquire) != generation {
        return Err(Error("Synthesis cancelled".into()));
    }
    Ok(())
}
