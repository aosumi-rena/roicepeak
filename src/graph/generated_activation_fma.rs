#![deny(unsafe_op_in_unsafe_fn)]

use super::{Arithmetic, Kind};
use std::arch::x86_64::*;

pub(super) fn apply(kind: Kind, values: &mut [f32]) -> bool {
    if Arithmetic::detected() != Arithmetic::Fused {
        return false;
    }
    // Feature detection precedes both vector variants.
    unsafe {
        match kind {
            Kind::Tanh => run::<true>(kind, values),
            Kind::Sigmoid => run::<false>(kind, values),
        }
    }
    true
}

#[target_feature(enable = "avx,fma")]
unsafe fn run<const TANH: bool>(kind: Kind, values: &mut [f32]) {
    let one = _mm_set1_ps(1.0);
    let zero = _mm_setzero_ps();
    let sign = _mm_set1_ps(-0.0);
    let [c, b, a] = kind.coefficients().map(|value| _mm_set1_ps(value));
    let evaluate = |x| {
        let absolute = _mm_andnot_ps(sign, x);
        let square = _mm_mul_ps(x, x);
        let polynomial = _mm_fmadd_ps(c, square, b);
        let polynomial = _mm_fmadd_ps(polynomial, absolute, a);
        let polynomial = _mm_fmadd_ps(polynomial, absolute, one);
        let estimate = _mm_rcp_ps(polynomial);
        let mask = if TANH {
            _mm_cmpnlt_ps(x, zero)
        } else {
            _mm_cmplt_ps(x, zero)
        };
        let selected = _mm_or_ps(_mm_and_ps(polynomial, mask), _mm_andnot_ps(mask, estimate));
        let square = _mm_mul_ps(selected, selected);
        if TANH {
            _mm_div_ps(_mm_sub_ps(square, one), _mm_add_ps(square, one))
        } else {
            _mm_rcp_ps(_mm_add_ps(square, one))
        }
    };
    let (groups, tail) = values.as_chunks_mut::<4>();
    for group in groups {
        // The load and store stay within one four-element array.
        unsafe {
            let lanes = _mm_loadu_ps(group.as_ptr());
            _mm_storeu_ps(group.as_mut_ptr(), evaluate(lanes));
        }
    }
    if !tail.is_empty() {
        let mut input = [0.0; 4];
        input[..tail.len()].copy_from_slice(tail);
        unsafe {
            let lanes = _mm_loadu_ps(input.as_ptr());
            _mm_storeu_ps(input.as_mut_ptr(), evaluate(lanes));
        }
        tail.copy_from_slice(&input[..tail.len()]);
    }
}
