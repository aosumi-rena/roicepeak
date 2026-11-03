const INVERSE: f64 = f64::from_bits(0x3fe4_5f30_6dc9_c883);

pub(super) fn scalar(x: f64) -> (f64, f64, u32) {
    let n = (x * INVERSE + 0.5).trunc() as u32;
    let count = f64::from(n);
    let mut head = x - count * f64::from_bits(0x3ff9_21fb_5440_0000);
    let mut tail = count * f64::from_bits(0x3dd0_b461_1a62_6331);
    let mut reduced = head - tail;
    let x_exponent = ((x.to_bits() >> 52) & 2047) as i32;
    let r_exponent = ((reduced.to_bits() >> 52) & 2047) as i32;
    if x_exponent - r_exponent > 15 {
        let old = head;
        let high = count * f64::from_bits(0x3dd0_b461_1a60_0000);
        let low = count * f64::from_bits(0x3ba3_198a_2e03_7073);
        head -= high;
        let compensation = (old - head) - high;
        tail = low - compensation;
        reduced = head - tail;
    }
    let error = (head - reduced) - tail;
    (reduced, error, n)
}

pub(super) fn fused(x: f64) -> (f64, f64, u32) {
    let magic = f64::from_bits(0x4338_0000_0000_0000);
    let count = x.mul_add(INVERSE, magic) - magic;
    let n = (count as u32) & 3;
    let t = (-count).mul_add(f64::from_bits(0x3ff9_21fb_5444_2d18), x);
    let tail_high = f64::from_bits(0x3c91_a626_3314_5c00);
    let w = count * tail_high;
    let v = count.mul_add(tail_high, -w);
    let y = t - w;
    let error = (t - y) - w;
    let r = (-count).mul_add(tail_high, t);
    let e = ((y - r) + error) - v;
    let e = (-count).mul_add(f64::from_bits(0x397b_839a_2520_49c0), e);
    (r, e, n)
}
