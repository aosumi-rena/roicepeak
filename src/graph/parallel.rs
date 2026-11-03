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

/// Independent row chunks with a separate workspace for each callback.
pub(super) struct FallibleRows {
    rows: usize,
    grain: usize,
    pool: Option<&'static ThreadPool>,
}

impl FallibleRows {
    pub(super) fn new(rows: usize, macs_per_row: usize) -> Self {
        if rows < 8 || rows.saturating_mul(macs_per_row) < MIN_PARALLEL_MACS {
            return Self {
                rows,
                grain: rows.max(1),
                pool: None,
            };
        }
        let Some(pool) = pool() else {
            return Self {
                rows,
                grain: rows.max(1),
                pool: None,
            };
        };
        let grain = rows
            .div_ceil(pool.current_num_threads() * 2)
            .max(MIN_TASK_MACS.div_ceil(macs_per_row.max(1)))
            .min(rows);
        Self {
            rows,
            grain,
            pool: if grain == rows { None } else { Some(pool) },
        }
    }

    /// Charge every scheduled workspace, including chunks run serially.
    pub(super) fn chunks(&self) -> usize {
        self.rows.div_ceil(self.grain)
    }

    pub(super) fn execute(
        &self,
        output: &mut [f32],
        channels: usize,
        execute: impl Fn(usize, &mut [f32]) -> crate::Result<()> + Sync,
    ) -> crate::Result<()> {
        if channels == 0 || output.len() % channels != 0 || output.len() / channels != self.rows {
            return Err(crate::invalid("row schedule dimensions"));
        }
        if self.rows == 0 {
            return Ok(());
        }
        let Some(pool) = self.pool else {
            return execute(0, output);
        };
        let chunk_elements = self
            .grain
            .checked_mul(channels)
            .ok_or_else(|| crate::invalid("row schedule size overflow"))?;
        pool.install(|| {
            output
                .par_chunks_mut(chunk_elements)
                .enumerate()
                .try_for_each(|(index, target)| execute(index * self.grain, target))
        })
    }
}
