//! A fixed 1024-complex transform with ordered F32 arithmetic.
mod first;
mod ordinary;
mod twisted;

pub(super) const WORDS: usize = 2048;

fn stage(values: &mut [f32], twisted_stage: bool, inverse_root: bool) {
    if values.len() == 32 {
        let input: &[f32; 32] = (&*values).try_into().expect("sixteen complex inputs");
        let output = if twisted_stage {
            twisted::apply(input)
        } else {
            ordinary::apply(input)
        };
        values.copy_from_slice(&output);
        return;
    }
    if twisted_stage {
        first::twisted(values);
    } else {
        first::ordinary(values, inverse_root);
    }
    let quarter = values.len() / 4;
    for (i, child) in values.chunks_exact_mut(quarter).enumerate() {
        stage(child, i == 1 || (i == 3 && twisted_stage), false);
    }
}

// Positive-sine when inverse=false, negative-sine when inverse=true; unnormalized.
pub(super) fn transform(values: &mut [f32; WORDS], inverse: bool, _fused: bool) {
    // The fixed twiddles round identically for both supported math branches.
    stage(values, false, inverse);
    for i in 0usize..WORDS / 2 {
        let j = i.reverse_bits() >> (usize::BITS - 10);
        if i < j {
            values.swap(2 * i, 2 * j);
            values.swap(2 * i + 1, 2 * j + 1);
        }
    }
    if inverse {
        for value in values.iter_mut().skip(1).step_by(2) {
            *value = f32::from_bits(value.to_bits() ^ 0x80000000);
        }
    }
}
