//! Ordered quantized products over independent output lanes.
#![cfg(target_arch = "x86_64")]

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
