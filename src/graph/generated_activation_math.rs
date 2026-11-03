//! Ordered lane equations for the generated x64 activation backend.
#![cfg(target_arch = "x86_64")]

use safe_arch::{
    add_m128, bitand_m128, bitandnot_m128, bitor_m128, cmp_lt_mask_m128, cmp_nlt_mask_m128,
    div_m128, m128, mul_m128, reciprocal_m128, sub_m128,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Tanh,
    Sigmoid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arithmetic {
    Separate,
    Fused,
}

impl Arithmetic {
    /// Default detected backend context; independent of CRT math dispatch flags.
    pub fn detected() -> Self {
        if std::arch::is_x86_feature_detected!("avx") && std::arch::is_x86_feature_detected!("fma")
        {
            Self::Fused
        } else {
            Self::Separate
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LengthMismatch;

#[derive(Debug, Clone, Copy)]
pub struct Kernel {
    arithmetic: Arithmetic,
}

impl Kernel {
    pub const fn new(arithmetic: Arithmetic) -> Self {
        Self { arithmetic }
    }

    pub fn scalar(self, kind: Kind, x: f32) -> f32 {
        self.lanes(kind, [x; 4])[0]
    }

    /// Logical lanes are independent; an incomplete final group uses owned padding.
    pub fn apply(
        self,
        kind: Kind,
        input: &[f32],
        output: &mut [f32],
    ) -> Result<(), LengthMismatch> {
        if input.len() != output.len() {
            return Err(LengthMismatch);
        }
        for (source, target) in input.chunks(4).zip(output.chunks_mut(4)) {
            let mut values = [0.0; 4];
            values[..source.len()].copy_from_slice(source);
            let result = self.lanes(kind, values);
            target.copy_from_slice(&result[..target.len()]);
        }
        Ok(())
    }

    fn lanes(self, kind: Kind, values: [f32; 4]) -> [f32; 4] {
        let x = m128::from_array(values);
        let one = m128::from_array([1.0; 4]);
        let zero = m128::from_array([0.0; 4]);
        let absolute = m128::from_bits(values.map(|v| v.to_bits() & 0x7fff_ffff));
        let square = mul_m128(x, x);
        let [c, b, a] = match kind {
            Kind::Tanh => [0x3e06_0807, 0x3f19_8f1d, 0x3f7b_fbbd],
            Kind::Sigmoid => [0x3c06_0568, 0x3e19_4af5, 0x3efb_fc65],
        }
        .map(f32::from_bits);
        let polynomial = match self.arithmetic {
            Arithmetic::Separate => {
                let p = add_m128(
                    mul_m128(m128::from_array([c; 4]), square),
                    m128::from_array([b; 4]),
                );
                let p = add_m128(mul_m128(p, absolute), m128::from_array([a; 4]));
                add_m128(mul_m128(p, absolute), one)
            }
            Arithmetic::Fused => {
                let square = square.to_array();
                let absolute = absolute.to_array();
                m128::from_array(std::array::from_fn(|i| {
                    let p = c.mul_add(square[i], b);
                    let p = p.mul_add(absolute[i], a);
                    p.mul_add(absolute[i], 1.0)
                }))
            }
        };
        let estimate = reciprocal_m128(polynomial);
        let mask = match kind {
            Kind::Tanh => cmp_nlt_mask_m128(x, zero),
            Kind::Sigmoid => cmp_lt_mask_m128(x, zero),
        };
        let selected = bitor_m128(
            bitand_m128(polynomial, mask),
            bitandnot_m128(mask, estimate),
        );
        let square = mul_m128(selected, selected);
        match kind {
            Kind::Tanh => div_m128(sub_m128(square, one), add_m128(square, one)),
            Kind::Sigmoid => reciprocal_m128(add_m128(square, one)),
        }
        .to_array()
    }
}
