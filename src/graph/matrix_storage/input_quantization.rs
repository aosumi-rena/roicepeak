#[cfg(target_arch = "x86_64")]
use safe_arch::{
    convert_to_i32_m128i_from_m128, m128, mul_m128, pack_i32_to_i16_m128i, set_splat_m128,
};

pub(super) fn quantize_into(input: &[f32], scale: f32, output: &mut [i16]) {
    debug_assert_eq!(input.len(), output.len());
    #[cfg(target_arch = "x86_64")]
    {
        let scale = set_splat_m128(scale);
        let grouped = input.len() / 8 * 8;
        for (source, target) in input[..grouped]
            .chunks_exact(8)
            .zip(output[..grouped].chunks_exact_mut(8))
        {
            target.copy_from_slice(&block(source.try_into().unwrap(), scale));
        }
        if grouped != input.len() {
            let remaining = input.len() - grouped;
            let mut source = [0.0; 8];
            source[..remaining].copy_from_slice(&input[grouped..]);
            let values = block(&source, scale);
            output[grouped..].copy_from_slice(&values[..remaining]);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    for (target, &value) in output.iter_mut().zip(input) {
        *target = scalar(value, scale);
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn block(input: &[f32; 8], scale: m128) -> [i16; 8] {
    let low = m128::from_array(input[..4].try_into().unwrap());
    let high = m128::from_array(input[4..].try_into().unwrap());
    let low = convert_to_i32_m128i_from_m128(mul_m128(low, scale));
    let high = convert_to_i32_m128i_from_m128(mul_m128(high, scale));
    pack_i32_to_i16_m128i(low, high).into()
}

#[cfg(not(target_arch = "x86_64"))]
fn scalar(value: f32, scale: f32) -> i16 {
    let scaled = value * scale;
    let integer = if !(-2147483648.0..2147483648.0).contains(&scaled) {
        i32::MIN
    } else {
        scaled.round_ties_even() as i32
    };
    integer.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}
