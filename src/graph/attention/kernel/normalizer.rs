use std::arch::x86_64::*;

#[derive(Clone, Copy)]
pub(super) enum Branch {
    AvxFma,
    AvxSeparate,
    SseSeparate,
}

pub(super) fn supported_width(width: usize) -> bool {
    width > 0 && width <= i32::MAX as usize - 7
}

pub(super) unsafe fn normalize_raw(
    branch: Branch,
    width: usize,
    values: *mut f32,
    extent: usize,
) -> bool {
    let lanes = match branch {
        Branch::SseSeparate => 4,
        _ => 8,
    };
    let Some(rounded) = width.checked_add(lanes - 1).map(|n| n / lanes * lanes) else {
        return false;
    };
    if !supported_width(width) || values.is_null() || extent < rounded {
        return false;
    }
    match branch {
        Branch::SseSeparate => unsafe { sse(width, values, values) },
        Branch::AvxSeparate => {
            if !is_x86_feature_detected!("avx") {
                return false;
            }
            unsafe { avx_separate(width, values, values) };
        }
        Branch::AvxFma => {
            if !is_x86_feature_detected!("avx") || !is_x86_feature_detected!("fma") {
                return false;
            }
            unsafe { avx_fma(width, values, values) };
        }
    }
    true
}

#[target_feature(enable = "sse2")]
unsafe fn sse(width: usize, source: *const f32, target: *mut f32) {
    unsafe {
        let blocks = width.div_ceil(4);
        let floor = _mm_set1_ps(f32::from_bits(0xe0ad78ec));
        let mut maximum = [floor; 2];
        for block in 0..blocks {
            let mut x = _mm_loadu_ps(source.add(block * 4));
            if block + 1 == blocks && width % 4 != 0 {
                let mask =
                    std::array::from_fn::<u32, 4, _>(|i| if i < width % 4 { u32::MAX } else { 0 });
                let tail =
                    std::array::from_fn::<u32, 4, _>(
                        |i| if i < width % 4 { 0 } else { 0xe0ad78ec },
                    );
                x = _mm_and_ps(x, _mm_castsi128_ps(_mm_loadu_si128(mask.as_ptr().cast())));
                x = _mm_add_ps(x, _mm_castsi128_ps(_mm_loadu_si128(tail.as_ptr().cast())));
            }
            let accumulator = if block + 1 == blocks && width % 4 != 0 {
                0
            } else {
                block & 1
            };
            maximum[accumulator] = _mm_max_ps(maximum[accumulator], x);
        }
        let m = _mm_max_ps(maximum[0], maximum[1]);
        let t = _mm_max_ps(_mm_shuffle_ps::<0x4e>(m, m), m);
        let maximum = _mm_max_ps(t, _mm_shuffle_ps::<0xb1>(t, t));
        let mut sums = [_mm_setzero_ps(); 2];
        for block in 0..blocks {
            let y = _mm_min_ps(
                _mm_sub_ps(maximum, _mm_loadu_ps(source.add(block * 4))),
                _mm_set1_ps(12.0),
            );
            let p = _mm_add_ps(
                _mm_mul_ps(_mm_set1_ps(f32::from_bits(0x3aec595a)), y),
                _mm_set1_ps(f32::from_bits(0xbd7f964e)),
            );
            let mut p = _mm_add_ps(_mm_mul_ps(p, y), _mm_set1_ps(1.0));
            p = _mm_mul_ps(p, p);
            p = _mm_mul_ps(p, p);
            p = _mm_mul_ps(p, p);
            p = _mm_mul_ps(p, p);
            if block + 1 == blocks && width % 4 != 0 {
                let mask =
                    std::array::from_fn::<u32, 4, _>(|i| if i < width % 4 { u32::MAX } else { 0 });
                p = _mm_and_ps(p, _mm_castsi128_ps(_mm_loadu_si128(mask.as_ptr().cast())));
            }
            _mm_storeu_ps(target.add(block * 4), p);
            sums[block & 1] = _mm_add_ps(sums[block & 1], p);
        }
        let sum = _mm_add_ps(sums[0], sums[1]);
        let t = _mm_add_ps(_mm_shuffle_ps::<0x4e>(sum, sum), sum);
        let reciprocal = _mm_rcp_ps(_mm_add_ps(t, _mm_shuffle_ps::<0xb1>(t, t)));
        for block in 0..blocks {
            _mm_storeu_ps(
                target.add(block * 4),
                _mm_mul_ps(reciprocal, _mm_loadu_ps(target.add(block * 4))),
            );
        }
    }
}

// The two AVX leaves differ only in the two polynomial multiply/add operations.
macro_rules! avx_body {
    ($width:ident, $source:ident, $target:ident, $fused:literal) => {{
        let blocks = $width.div_ceil(8);
        let floor = _mm256_set1_ps(f32::from_bits(0xe0ad78ec));
        let mut maximum = [floor; 2];
        for block in 0..blocks {
            let mut x = _mm256_loadu_ps($source.add(block * 8));
            if block + 1 == blocks && $width % 8 != 0 {
                let mask =
                    std::array::from_fn::<u32, 8, _>(|i| if i < $width % 8 { u32::MAX } else { 0 });
                let tail =
                    std::array::from_fn::<u32, 8, _>(
                        |i| if i < $width % 8 { 0 } else { 0xe0ad78ec },
                    );
                x = _mm256_and_ps(
                    x,
                    _mm256_castsi256_ps(_mm256_loadu_si256(mask.as_ptr().cast())),
                );
                x = _mm256_add_ps(
                    x,
                    _mm256_castsi256_ps(_mm256_loadu_si256(tail.as_ptr().cast())),
                );
            }
            let accumulator = if block + 1 == blocks && $width % 8 != 0 {
                0
            } else {
                block & 1
            };
            maximum[accumulator] = _mm256_max_ps(maximum[accumulator], x);
        }
        let m = _mm256_max_ps(maximum[0], maximum[1]);
        let m = _mm256_max_ps(m, _mm256_permute_ps::<0xb1>(m));
        let m = _mm256_max_ps(m, _mm256_permute_ps::<0x4e>(m));
        let maximum = _mm256_max_ps(_mm256_permute2f128_ps::<1>(m, m), m);
        let mut sums = [_mm256_setzero_ps(); 2];
        for block in 0..blocks {
            let y = _mm256_min_ps(
                _mm256_sub_ps(maximum, _mm256_loadu_ps($source.add(block * 8))),
                _mm256_set1_ps(12.0),
            );
            let c2 = _mm256_set1_ps(f32::from_bits(0x3aec595a));
            let c1 = _mm256_set1_ps(f32::from_bits(0xbd7f964e));
            let p;
            let mut p2;
            if $fused {
                p = _mm256_fmadd_ps(c2, y, c1);
                p2 = _mm256_fmadd_ps(p, y, _mm256_set1_ps(1.0));
            } else {
                p = _mm256_add_ps(_mm256_mul_ps(c2, y), c1);
                p2 = _mm256_add_ps(_mm256_mul_ps(p, y), _mm256_set1_ps(1.0));
            }
            p2 = _mm256_mul_ps(p2, p2);
            p2 = _mm256_mul_ps(p2, p2);
            p2 = _mm256_mul_ps(p2, p2);
            p2 = _mm256_mul_ps(p2, p2);
            if block + 1 == blocks && $width % 8 != 0 {
                let mask =
                    std::array::from_fn::<u32, 8, _>(|i| if i < $width % 8 { u32::MAX } else { 0 });
                p2 = _mm256_and_ps(
                    p2,
                    _mm256_castsi256_ps(_mm256_loadu_si256(mask.as_ptr().cast())),
                );
            }
            _mm256_storeu_ps($target.add(block * 8), p2);
            sums[block & 1] = _mm256_add_ps(sums[block & 1], p2);
        }
        let sum = _mm256_add_ps(sums[0], sums[1]);
        let sum = _mm256_add_ps(sum, _mm256_permute_ps::<0xb1>(sum));
        let sum = _mm256_add_ps(sum, _mm256_permute_ps::<0x4e>(sum));
        let sum = _mm256_add_ps(_mm256_permute2f128_ps::<1>(sum, sum), sum);
        let reciprocal = _mm256_rcp_ps(sum);
        for block in 0..blocks {
            _mm256_storeu_ps(
                $target.add(block * 8),
                _mm256_mul_ps(reciprocal, _mm256_loadu_ps($target.add(block * 8))),
            );
        }
    }};
}

#[target_feature(enable = "avx")]
unsafe fn avx_separate(width: usize, source: *const f32, target: *mut f32) {
    unsafe { avx_body!(width, source, target, false) };
}

#[target_feature(enable = "avx,fma")]
unsafe fn avx_fma(width: usize, source: *const f32, target: *mut f32) {
    unsafe { avx_body!(width, source, target, true) };
}
