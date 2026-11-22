//! Ordered quantized products over independent output lanes.
#![cfg(target_arch = "x86_64")]
#![deny(unsafe_op_in_unsafe_fn)]

use safe_arch::{
    add_i16_m128i, add_saturating_i16_m128i, m128i, mul_i16_keep_high_m128i,
    mul_i16_keep_low_m128i, set_splat_i16_m128i, shl_imm_u16_m128i, shr_imm_u16_m128i,
};

#[inline(always)]
fn rounded_lanes(a: m128i, b: m128i) -> m128i {
    let high = mul_i16_keep_high_m128i(a, b);
    let low = mul_i16_keep_low_m128i(a, b);
    let rounding = shr_imm_u16_m128i::<1>(add_i16_m128i(
        shr_imm_u16_m128i::<14>(low),
        set_splat_i16_m128i(1),
    ));
    add_i16_m128i(shl_imm_u16_m128i::<1>(high), rounding)
}

#[inline(always)]
fn load_seed(values: &[i16]) -> m128i {
    m128i::from(<[i16; 8]>::try_from(values).unwrap())
}

#[inline(always)]
fn load(values: &[i8]) -> m128i {
    m128i::from(std::array::from_fn::<i16, 8, _>(|i| {
        i16::from(values[i]) * 256
    }))
}

pub fn integer_product(weights: &[i8], input: &[i16], output: &mut [i16], add: bool) {
    let width = output.len();
    debug_assert_eq!(weights.len(), input.len() * width);
    if input.is_empty() {
        return;
    }
    if width != 0
        && width.is_multiple_of(16)
        && input.len().checked_mul(width) == Some(weights.len())
        && std::arch::is_x86_feature_detected!("avx2")
    {
        // Each vector load reads one complete group of 16 coefficients in its row.
        unsafe {
            if width.is_multiple_of(64) {
                product_avx2::<4>(weights, input, output, add);
            } else {
                product_avx2::<1>(weights, input, output, add);
            }
        }
        return;
    }
    let grouped = width / 8 * 8;
    for first in (0..grouped).step_by(8) {
        let mut sum = rounded_lanes(
            set_splat_i16_m128i(input[0]),
            load(&weights[first..first + 8]),
        );
        if add {
            sum = add_saturating_i16_m128i(load_seed(&output[first..first + 8]), sum);
        }
        for (index, &value) in input.iter().enumerate().skip(1) {
            let at = index * width + first;
            let product = rounded_lanes(set_splat_i16_m128i(value), load(&weights[at..at + 8]));
            sum = add_saturating_i16_m128i(sum, product);
        }
        let values: [i16; 8] = sum.into();
        output[first..first + 8].copy_from_slice(&values);
    }
    for lane in grouped..width {
        for (index, &value) in input.iter().enumerate() {
            let weight = weights[index * width + lane];
            let product = ((i32::from(value) * (i32::from(weight) * 256) + 16384) >> 15) as i16;
            output[lane] = if index == 0 && !add {
                product
            } else {
                output[lane].saturating_add(product)
            };
        }
    }
}

#[target_feature(enable = "avx2")]
unsafe fn product_avx2<const VECTORS: usize>(
    weights: &[i8],
    input: &[i16],
    output: &mut [i16],
    add: bool,
) {
    use std::arch::x86_64::*;

    let width = output.len();
    for (index, lanes) in output.chunks_exact_mut(16 * VECTORS).enumerate() {
        let first = index * 16 * VECTORS;
        let target = lanes.as_mut_ptr();
        let mut sums = [_mm256_setzero_si256(); VECTORS];
        if add {
            for (lane, sum) in sums.iter_mut().enumerate() {
                *sum = unsafe { _mm256_loadu_si256(target.add(lane * 16).cast()) };
            }
        }
        for (&value, row) in input.iter().zip(weights.chunks_exact(width)) {
            let broadcast = _mm256_set1_epi16(value);
            let source = unsafe { row.as_ptr().add(first) };
            for (lane, sum) in sums.iter_mut().enumerate() {
                let packed = unsafe { _mm_loadu_si128(source.add(lane * 16).cast()) };
                let coefficients = _mm256_slli_epi16::<8>(_mm256_cvtepi8_epi16(packed));
                *sum = _mm256_adds_epi16(*sum, _mm256_mulhrs_epi16(broadcast, coefficients));
            }
        }
        for (lane, sum) in sums.into_iter().enumerate() {
            unsafe { _mm256_storeu_si256(target.add(lane * 16).cast(), sum) };
        }
    }
}
