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
fn load(values: &[i16]) -> m128i {
    m128i::from(<[i16; 8]>::try_from(values).unwrap())
}

pub fn integer_product(weights: &[i16], input: &[i16], output: &mut [i16], add: bool) {
    let width = output.len();
    debug_assert_eq!(weights.len(), input.len() * width);
    if input.is_empty() {
        return;
    }
    let first_vector_tail = if width >= 16
        && input.len().checked_mul(width) == Some(weights.len())
        && std::arch::is_x86_feature_detected!("avx2")
    {
        // Only complete lanes are read or written by the vector loop.
        unsafe { product_avx2(weights, input, output, add) };
        width / 16 * 16
    } else {
        0
    };
    let grouped = width / 8 * 8;
    for first in (first_vector_tail..grouped).step_by(8) {
        let mut sum = rounded_lanes(
            set_splat_i16_m128i(input[0]),
            load(&weights[first..first + 8]),
        );
        if add {
            sum = add_saturating_i16_m128i(load(&output[first..first + 8]), sum);
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
            let product = ((i32::from(value) * i32::from(weight) + 16384) >> 15) as i16;
            output[lane] = if index == 0 && !add {
                product
            } else {
                output[lane].saturating_add(product)
            };
        }
    }
}

#[target_feature(enable = "avx2")]
unsafe fn product_avx2(weights: &[i16], input: &[i16], output: &mut [i16], add: bool) {
    use std::arch::x86_64::*;

    let width = output.len();
    for (index, lanes) in output.chunks_exact_mut(16).enumerate() {
        let destination = lanes.as_mut_ptr().cast();
        let mut sum = if add {
            unsafe { _mm256_loadu_si256(destination) }
        } else {
            _mm256_setzero_si256()
        };
        for (&value, row) in input.iter().zip(weights.chunks_exact(width)) {
            let coefficients = unsafe { _mm256_loadu_si256(row.as_ptr().add(index * 16).cast()) };
            let product = _mm256_mulhrs_epi16(_mm256_set1_epi16(value), coefficients);
            sum = _mm256_adds_epi16(sum, product);
        }
        unsafe { _mm256_storeu_si256(destination, sum) };
    }
}
