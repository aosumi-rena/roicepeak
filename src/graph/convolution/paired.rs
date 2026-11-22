#![deny(unsafe_op_in_unsafe_fn)]

pub(super) fn row(
    coefficients: &[f32],
    stride: usize,
    inputs: usize,
    taps: usize,
    queued: [&[f32]; 2],
    output: [&mut [f32]; 2],
) -> bool {
    let width = output[0].len();
    if width == 0
        || stride < width
        || !stride.is_multiple_of(8)
        || width != output[1].len()
        || inputs == 0
        || taps == 0
        || inputs.checked_mul(taps) != Some(queued[0].len())
        || queued[0].len() != queued[1].len()
        || queued[0].len().checked_mul(stride) != Some(coefficients.len())
    {
        return false;
    }
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx") && std::arch::is_x86_feature_detected!("fma") {
        let [front, next] = output;
        let mut begin = 0;
        while begin < inputs {
            let remaining = inputs - begin;
            let count = if remaining >= 128 { 64 } else { remaining };
            for tap in 0..taps {
                let first = tap * inputs + begin;
                let last = first + count;
                // The shape checks cover all loads in each 32-channel group.
                unsafe {
                    product(
                        &coefficients[first * stride..last * stride],
                        stride,
                        &queued[0][first..last],
                        &queued[1][first..last],
                        front,
                        next,
                        begin != 0 || tap != 0,
                    );
                }
            }
            begin += count;
        }
        return true;
    }
    false
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx,fma")]
unsafe fn product(
    coefficients: &[f32],
    stride: usize,
    left: &[f32],
    right: &[f32],
    first: &mut [f32],
    second: &mut [f32],
    add: bool,
) {
    use std::arch::x86_64::*;

    let complete = first.len() / 32 * 32;
    for channel in (0..complete).step_by(32) {
        let mut a = [_mm256_setzero_ps(); 4];
        let mut b = a;
        let p = unsafe { first.as_mut_ptr().add(channel) };
        let q = unsafe { second.as_mut_ptr().add(channel) };
        if add {
            for i in 0..4 {
                a[i] = unsafe { _mm256_loadu_ps(p.add(i * 8)) };
                b[i] = unsafe { _mm256_loadu_ps(q.add(i * 8)) };
            }
        }
        for (i, (&l, &r)) in left.iter().zip(right).enumerate() {
            let l = _mm256_set1_ps(l);
            let r = _mm256_set1_ps(r);
            let row = unsafe { coefficients.as_ptr().add(i * stride + channel) };
            for j in 0..4 {
                let weight = unsafe { _mm256_loadu_ps(row.add(j * 8)) };
                a[j] = _mm256_fmadd_ps(weight, l, a[j]);
                b[j] = _mm256_fmadd_ps(weight, r, b[j]);
            }
        }
        for i in 0..4 {
            unsafe {
                _mm256_storeu_ps(p.add(i * 8), a[i]);
                _mm256_storeu_ps(q.add(i * 8), b[i]);
            }
        }
    }
    for channel in (complete..first.len()).step_by(8) {
        let end = (channel + 8).min(first.len());
        let front = &mut first[channel..end];
        let next = &mut second[channel..end];
        let mut front_buffer = [0.0; 8];
        let mut next_buffer = [0.0; 8];
        if add {
            front_buffer[..front.len()].copy_from_slice(front);
            next_buffer[..next.len()].copy_from_slice(next);
        }
        let mut a = unsafe { _mm256_loadu_ps(front_buffer.as_ptr()) };
        let mut b = unsafe { _mm256_loadu_ps(next_buffer.as_ptr()) };
        for (i, (&l, &r)) in left.iter().zip(right).enumerate() {
            let weight =
                unsafe { _mm256_loadu_ps(coefficients.as_ptr().add(i * stride + channel)) };
            a = _mm256_fmadd_ps(weight, _mm256_set1_ps(l), a);
            b = _mm256_fmadd_ps(weight, _mm256_set1_ps(r), b);
        }
        unsafe {
            _mm256_storeu_ps(front_buffer.as_mut_ptr(), a);
            _mm256_storeu_ps(next_buffer.as_mut_ptr(), b);
        }
        front.copy_from_slice(&front_buffer[..front.len()]);
        next.copy_from_slice(&next_buffer[..next.len()]);
    }
}
