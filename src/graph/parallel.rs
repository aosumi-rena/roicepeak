//! Independent output-row work; inner reductions remain entirely sequential.
//! Uses a bounded persistent worker pool.
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};
use std::sync::OnceLock;

const MAX_THREADS: usize = 8;
const MIN_PARALLEL_MACS: usize = 2 * 1024 * 1024;
const MIN_TASK_MACS: usize = 256 * 1024;
static POOL: OnceLock<Option<ThreadPool>> = OnceLock::new();

fn pool() -> Option<&'static ThreadPool> {
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .ok()?
            .get()
            .min(MAX_THREADS);
        if threads <= 1 {
            return None;
        }
        ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|index| format!("roicepeak-compute-{index}"))
            .build()
            .ok()
    })
    .as_ref()
}

/// The caller validates dimensions and spends its complete budget before entry.
/// A failed pool initialization falls back to the same serial row kernel.
pub(super) fn for_rows(
    output: &mut [f32],
    channels: usize,
    macs_per_row: usize,
    execute: impl Fn(usize, &mut [f32]) + Sync,
) {
    debug_assert!(channels > 0 && output.len() % channels == 0);
    let rows = output.len() / channels;
    if rows < 8 || rows.saturating_mul(macs_per_row) < MIN_PARALLEL_MACS {
        execute(0, output);
        return;
    }
    let Some(pool) = pool() else {
        execute(0, output);
        return;
    };
    let grain = rows
        .div_ceil(pool.current_num_threads() * 2)
        .max(MIN_TASK_MACS.div_ceil(macs_per_row.max(1)))
        .min(rows);
    if grain == rows {
        execute(0, output);
        return;
    }
    pool.install(|| {
        output
            .par_chunks_mut(grain * channels)
            .enumerate()
            .for_each(|(index, target)| execute(index * grain, target));
    });
}
