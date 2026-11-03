//! A fixed 1024-point real transform with F32 butterflies and packed complex bins.
mod first;
mod fixed;
mod recombine;

pub(super) const SIZE: usize = 1024;

fn stage(values: &mut [f32], twisted: bool, inverse_root: bool) {
    if values.len() == 16 {
        let input: &[f32; 16] = (&*values).try_into().expect("eight complex inputs");
        let output = if twisted {
            fixed::twisted(input)
        } else {
            fixed::ordinary(input)
        };
        values.copy_from_slice(&output);
        return;
    }
    if twisted {
        first::twisted(values);
    } else {
        first::ordinary(values, inverse_root);
    }
    let quarter = values.len() / 4;
    for (i, child) in values.chunks_exact_mut(quarter).enumerate() {
        stage(child, i == 1 || (i == 3 && twisted), false);
    }
}

fn reorder(values: &mut [f32; SIZE], inverse: bool) {
    for i in 0usize..SIZE / 2 {
        let j = i.reverse_bits() >> (usize::BITS - 9);
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

pub(super) fn forward(values: &mut [f32; SIZE]) {
    stage(values, false, false);
    reorder(values, false);
    recombine::apply(values, false);
    let difference = values[0] - values[1];
    values[0] += values[1];
    values[1] = difference;
}

// The inverse is unnormalized: inverse(forward(x)) = SIZE/2 * x.
pub(super) fn inverse(values: &mut [f32; SIZE]) {
    let half = (values[0] - values[1]) * 0.5;
    values[1] = half;
    values[0] -= half;
    recombine::apply(values, true);
    stage(values, false, true);
    reorder(values, true);
}

pub(super) fn multiply(values: &mut [f32; SIZE], response: &[f32; SIZE]) {
    values[0] = response[0] * values[0];
    values[1] = response[1] * values[1];
    for i in 1..SIZE / 2 {
        let (ar, ai, br, bi) = (
            values[2 * i],
            values[2 * i + 1],
            response[2 * i],
            response[2 * i + 1],
        );
        let imag_left = ar * bi;
        let real_right = bi * ai;
        let real_left = ar * br;
        let real = real_left - real_right;
        let imag_right = ai * br;
        values[2 * i] = real;
        values[2 * i + 1] = imag_left + imag_right;
    }
}
