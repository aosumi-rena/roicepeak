//! Bounded integer phase reduction for finite F32 arguments.

// Generated from floor((2 / pi) * 2^306), stored least-significant byte first.
const INVERSE_PI: [u8; 48] = [
    0x37, 0x09, 0xe9, 0xb8, 0x91, 0xdc, 0x86, 0x15, 0xef, 0x7a, 0xaf, 0x8e, 0x45, 0xf9, 0x07, 0x41,
    0x0e, 0xf1, 0x64, 0x56, 0x8a, 0x6d, 0x03, 0x77, 0xd3, 0xd4, 0x47, 0x5f, 0x9d, 0xf0, 0xa7, 0x54,
    0x10, 0x39, 0xb9, 0x0d, 0xe6, 0x8b, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[derive(Clone, Copy)]
struct Wide([u64; 3]);

impl Wide {
    fn highest(self) -> Option<u32> {
        for i in (0..3).rev() {
            if self.0[i] != 0 {
                return Some(i as u32 * 64 + 63 - self.0[i].leading_zeros());
            }
        }
        None
    }

    fn shifted_word(self, shift: u32) -> u64 {
        let i = (shift / 64) as usize;
        let offset = shift % 64;
        let mut value = self.0[i] >> offset;
        if offset != 0 && i + 1 < 3 {
            value |= self.0[i + 1] << (64 - offset);
        }
        value
    }

    fn below(self, bits: u32) -> Self {
        Self(std::array::from_fn(|i| {
            let start = i as u32 * 64;
            if bits <= start {
                0
            } else if bits < start + 64 {
                self.0[i] & ((1u64 << (bits - start)) - 1)
            } else {
                self.0[i]
            }
        }))
    }

    fn fraction(self, denominator_bits: u32, negative: bool) -> f64 {
        let sign = u64::from(negative) << 63;
        let Some(highest) = self.highest() else {
            return f64::from_bits(sign);
        };
        let exponent = highest as i32 - denominator_bits as i32;
        let significant = if highest >= 52 {
            self.shifted_word(highest - 52)
        } else {
            self.0[0] << (52 - highest)
        };
        f64::from_bits(sign | (((exponent + 1023) as u64) << 52) | (significant & ((1 << 52) - 1)))
    }
}

fn table_word(offset: usize) -> u64 {
    u64::from_le_bytes(
        INVERSE_PI[offset..offset + 8]
            .try_into()
            .expect("bounded phase table offset"),
    )
}

pub(super) fn large(value: f64, compensated: bool) -> (u32, f64) {
    debug_assert!(value.is_finite() && value > 0.0);
    let bits = value.to_bits();
    let exponent = (((bits >> 52) & 2047) as i32) - 1023;
    debug_assert!((18..=127).contains(&exponent));
    let mantissa = (bits & ((1 << 52) - 1)) | (1 << 52);
    let offset = 22 - ((exponent as usize) >> 3);
    let first = u128::from(table_word(offset)) * u128::from(mantissa);
    let second = u128::from(table_word(offset + 8)) * u128::from(mantissa);
    let third = u128::from(table_word(offset + 16)) * u128::from(mantissa);
    let (middle, carry) = (second as u64).overflowing_add((first >> 64) as u64);
    let high = ((second >> 64) as u64)
        .wrapping_add(u64::from(carry))
        .wrapping_add(third as u64);
    let shift = 54 - (exponent as u32 & 7);
    let negative = (high >> (shift - 1)) & 1 != 0;
    let quadrant = (((high >> shift) + u64::from(negative)) & 3) as u32;
    let mut words = [first as u64, middle, high];
    if negative {
        words = words.map(|word| !word);
    }
    let denominator_bits = 128 + shift;
    let fraction = Wide(words).below(denominator_bits);
    let high = fraction.fraction(denominator_bits, negative);
    let half_pi = f64::from_bits(0x3ff9_21fb_5444_2d18);
    if !compensated {
        return (quadrant, high * half_pi);
    }
    let discarded = fraction.highest().unwrap_or(0).saturating_sub(52);
    let low = fraction
        .below(discarded)
        .fraction(denominator_bits, negative);
    let head = f64::from_bits(high.to_bits() & 0xffff_ffff_f800_0000);
    let tail = high - head;
    let product = high * half_pi;
    let pi_head = f64::from_bits(0x3ff9_21fb_5000_0000);
    let pi_tail = f64::from_bits(0x3e51_10b4_6000_0000);
    let mut correction = head * pi_head - product;
    correction = tail.mul_add(pi_head, correction);
    correction = head.mul_add(pi_tail, correction);
    correction = tail.mul_add(pi_tail, correction);
    let low_product = high.mul_add(f64::from_bits(0x3c91_a626_3314_5c06), low * half_pi);
    correction += low_product;
    (quadrant, product + correction)
}
