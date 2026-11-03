#![deny(unsafe_op_in_unsafe_fn)]

/// Input-major coefficients with a channel stride divisible by eight.
pub fn product_into(
    coefficients: &[f32],
    stride: usize,
    input: &[f32],
    output: &mut [f32],
    add: bool,
) -> bool {
    if stride == 0
        || stride % 8 != 0
        || output.is_empty()
        || output.len() > stride
        || input.len().checked_mul(stride) != Some(coefficients.len())
    {
        return false;
    }
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx") && std::arch::is_x86_feature_detected!("fma") {
        // The shape checks cover every coefficient load; partial outputs use owned scratch.
        unsafe { product_avx(coefficients, stride, input, output, add) };
        return true;
    }
    false
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx,fma")]
unsafe fn product_avx(
    coefficients: &[f32],
    stride: usize,
    input: &[f32],
    output: &mut [f32],
    add: bool,
) {
    use std::arch::x86_64::*;

    let mut channel = 0;
    while channel + 32 <= output.len() {
        let destination = unsafe { output.as_mut_ptr().add(channel) };
        let mut a = _mm256_setzero_ps();
        let mut b = a;
        let mut c = a;
        let mut d = a;
        if add {
            unsafe {
                a = _mm256_loadu_ps(destination);
                b = _mm256_loadu_ps(destination.add(8));
                c = _mm256_loadu_ps(destination.add(16));
                d = _mm256_loadu_ps(destination.add(24));
            }
        }
        for (i, &value) in input.iter().enumerate() {
            let source = unsafe { coefficients.as_ptr().add(i * stride + channel) };
            let x = _mm256_set1_ps(value);
            unsafe {
                a = _mm256_fmadd_ps(_mm256_loadu_ps(source), x, a);
                b = _mm256_fmadd_ps(_mm256_loadu_ps(source.add(8)), x, b);
                c = _mm256_fmadd_ps(_mm256_loadu_ps(source.add(16)), x, c);
                d = _mm256_fmadd_ps(_mm256_loadu_ps(source.add(24)), x, d);
            }
        }
        unsafe {
            _mm256_storeu_ps(destination, a);
            _mm256_storeu_ps(destination.add(8), b);
            _mm256_storeu_ps(destination.add(16), c);
            _mm256_storeu_ps(destination.add(24), d);
        }
        channel += 32;
    }
    while channel + 8 <= output.len() {
        let destination = unsafe { output.as_mut_ptr().add(channel) };
        let mut sum = if add {
            unsafe { _mm256_loadu_ps(destination) }
        } else {
            _mm256_setzero_ps()
        };
        for (i, &value) in input.iter().enumerate() {
            let source = unsafe { coefficients.as_ptr().add(i * stride + channel) };
            sum = _mm256_fmadd_ps(
                unsafe { _mm256_loadu_ps(source) },
                _mm256_set1_ps(value),
                sum,
            );
        }
        unsafe { _mm256_storeu_ps(destination, sum) };
        channel += 8;
    }
    if channel < output.len() {
        let tail = &mut output[channel..];
        let mut scratch = [0.0; 8];
        if add {
            scratch[..tail.len()].copy_from_slice(tail);
        }
        let mut sum = unsafe { _mm256_loadu_ps(scratch.as_ptr()) };
        for (i, &value) in input.iter().enumerate() {
            let source = unsafe { coefficients.as_ptr().add(i * stride + channel) };
            sum = _mm256_fmadd_ps(
                unsafe { _mm256_loadu_ps(source) },
                _mm256_set1_ps(value),
                sum,
            );
        }
        unsafe { _mm256_storeu_ps(scratch.as_mut_ptr(), sum) };
        tail.copy_from_slice(&scratch[..tail.len()]);
    }
}
