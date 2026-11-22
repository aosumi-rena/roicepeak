use super::{Output, Prepared, RenderedAudio};
use crate::app::render::{RenderProfile, SAMPLE_RATE, cache::Cache, worker::merge_operators};
use roicepeak::{Result, graph::profiling};

const MAX_CONCURRENT_SAMPLES: usize = 16 * SAMPLE_RATE as usize;

pub(in crate::app::render) fn finish(
    mut prepared: Vec<Prepared>,
    cache: &mut Cache,
    profile: &mut RenderProfile,
    generation: u64,
    check: &(impl Fn() -> Result<()> + Sync),
) -> Result<Vec<Output>> {
    let keep = |cache: &mut Cache, rendered: RenderedAudio| {
        if let Some(base) = rendered.baseline {
            cache.remember(base, generation);
        }
        rendered.output
    };
    let concurrent = matches!(
        prepared.as_slice(),
        [Prepared::Audio(first), Prepared::Audio(second)]
            if first.phones.samples.saturating_add(second.phones.samples) <= MAX_CONCURRENT_SAMPLES
    );
    if !concurrent {
        return prepared
            .into_iter()
            .map(|unit| match unit {
                Prepared::Ready(output) => Ok(output),
                Prepared::Audio(audio) => {
                    let rendered = audio.render(profile, check, None)?;
                    Ok(keep(cache, rendered))
                }
            })
            .collect();
    }

    let (Some(Prepared::Audio(second)), Some(Prepared::Audio(first))) =
        (prepared.pop(), prepared.pop())
    else {
        unreachable!("qualified render batch");
    };
    std::thread::scope(|scope| {
        let parallel = scope.spawn(|| {
            let mut profile = RenderProfile::default();
            let (rendered, operators) =
                profiling::capture(|| second.render(&mut profile, check, None));
            (rendered, profile, operators)
        });
        let left = first.render(profile, check, None);
        let (right, other, operators) = parallel
            .join()
            .unwrap_or_else(|error| std::panic::resume_unwind(error));
        profile.baseline_calls.merge(other.baseline_calls);
        profile.edited_calls.merge(other.edited_calls);
        merge_operators(profile, operators);
        check()?;
        Ok(vec![keep(cache, left?), keep(cache, right?)])
    })
}
