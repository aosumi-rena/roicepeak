//! Optional operator wall-time accounting on the inference control thread.
//! This records no tensor values and does not alter graph evaluation order.
use std::{
    cell::RefCell,
    collections::BTreeMap,
    marker::PhantomData,
    rc::Rc,
    time::{Duration, Instant},
};

const MAX_GROUPS: usize = 64;

#[derive(Clone, Debug, Default)]
pub struct OperatorTiming {
    pub kind: &'static str,
    pub input_channels: usize,
    pub maximum_output_channels: usize,
    pub calls: u64,
    pub failures: u64,
    pub input_elements: u64,
    pub output_elements: u64,
    pub inclusive: Duration,
    pub exclusive: Duration,
    pub inclusive_multiply_adds: u64,
    pub exclusive_multiply_adds: u64,
}

#[derive(Default)]
struct Collector {
    groups: BTreeMap<(&'static str, usize), OperatorTiming>,
    stack: Vec<Frame>,
}
struct Frame {
    kind: &'static str,
    channels: usize,
    input_elements: usize,
    started: Instant,
    children: Duration,
    child_macs: u64,
    budget_before: usize,
    budget_after: usize,
    output_elements: usize,
    output_channels: usize,
    succeeded: bool,
}

thread_local! {
    static ACTIVE: RefCell<Option<Collector>> = const { RefCell::new(None) };
}

/// A nested capture is isolated and restores the surrounding capture on exit.
/// Panics also restore the previous collector; no timing state survives a job.
/// An unwinding capture discards its partial timings. Spans cannot recover the
/// final remaining budget during a panic; budget deltas cover ordinary Results.
pub fn capture<T>(run: impl FnOnce() -> T) -> (T, Vec<OperatorTiming>) {
    struct Restore {
        previous: Option<Collector>,
        finished: bool,
    }
    impl Drop for Restore {
        fn drop(&mut self) {
            if !self.finished {
                ACTIVE.with(|active| *active.borrow_mut() = self.previous.take());
            }
        }
    }
    let mut restore = Restore {
        previous: ACTIVE.with(|active| active.replace(Some(Collector::default()))),
        finished: false,
    };
    let output = run();
    let collected = ACTIVE.with(|active| active.replace(restore.previous.take()));
    restore.finished = true;
    let mut timings: Vec<_> = collected
        .into_iter()
        .flat_map(|collector| collector.groups.into_values())
        .collect();
    timings.sort_by(|a, b| {
        b.exclusive
            .cmp(&a.exclusive)
            .then(a.kind.cmp(b.kind))
            .then(a.input_channels.cmp(&b.input_channels))
    });
    (output, timings)
}

pub(super) struct Span {
    depth: usize,
    // A span belongs to the control thread's collector, never a worker row task.
    _thread: PhantomData<Rc<()>>,
}

pub(super) fn enter(
    kind: &'static str,
    rows: usize,
    channels: usize,
    remaining_macs: usize,
) -> Option<Span> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        let collector = active.as_mut()?;
        let depth = collector.stack.len();
        collector.stack.push(Frame {
            kind,
            channels,
            input_elements: rows.saturating_mul(channels),
            started: Instant::now(),
            children: Duration::ZERO,
            child_macs: 0,
            budget_before: remaining_macs,
            budget_after: remaining_macs,
            output_elements: 0,
            output_channels: 0,
            succeeded: false,
        });
        Some(Span {
            depth,
            _thread: PhantomData,
        })
    })
}

impl Span {
    /// Finalize the budget on both Result paths; failed calls have no output.
    pub(super) fn finish(self, output: Option<(usize, usize)>, remaining_macs: usize) {
        ACTIVE.with(|active| {
            if let Some(frame) = active
                .borrow_mut()
                .as_mut()
                .and_then(|collector| collector.stack.get_mut(self.depth))
            {
                frame.budget_after = remaining_macs;
                if let Some((rows, channels)) = output {
                    frame.succeeded = true;
                    frame.output_elements = rows.saturating_mul(channels);
                    frame.output_channels = channels;
                }
            }
        });
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            let mut active = active.borrow_mut();
            let Some(collector) = active.as_mut() else {
                return;
            };
            if collector.stack.len() != self.depth + 1 {
                return;
            }
            let Some(frame) = collector.stack.pop() else {
                return;
            };
            let elapsed = frame.started.elapsed();
            let macs = frame.budget_before.saturating_sub(frame.budget_after) as u64;
            if let Some(parent) = collector.stack.last_mut() {
                parent.children += elapsed;
                parent.child_macs = parent.child_macs.saturating_add(macs);
            }
            let requested = (frame.kind, frame.channels);
            let key = if collector.groups.contains_key(&requested)
                || collector.groups.len() < MAX_GROUPS - 1
            {
                requested
            } else {
                ("other_shapes", 0)
            };
            let entry = collector
                .groups
                .entry(key)
                .or_insert_with(|| OperatorTiming {
                    kind: key.0,
                    input_channels: key.1,
                    ..Default::default()
                });
            entry.calls += 1;
            entry.failures += u64::from(!frame.succeeded);
            entry.maximum_output_channels =
                entry.maximum_output_channels.max(frame.output_channels);
            entry.input_elements = entry
                .input_elements
                .saturating_add(frame.input_elements as u64);
            entry.output_elements = entry
                .output_elements
                .saturating_add(frame.output_elements as u64);
            entry.inclusive += elapsed;
            entry.exclusive += elapsed.saturating_sub(frame.children);
            entry.inclusive_multiply_adds = entry.inclusive_multiply_adds.saturating_add(macs);
            entry.exclusive_multiply_adds = entry
                .exclusive_multiply_adds
                .saturating_add(macs.saturating_sub(frame.child_macs));
        });
    }
}
