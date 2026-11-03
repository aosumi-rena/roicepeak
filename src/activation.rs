//! Scalar rational sigmoid with fixed evaluation order.

pub fn sigmoid(x: f32) -> f32 {
    const N: [u32; 11] = [
        0x43040000, 0x46061000, 0x48aff500, 0x4b24f5b0, 0x4d66f190, 0x4f7560a9, 0x51452dac,
        0x52ea263d, 0x54431fdd, 0x554ce175, 0x55cce175,
    ];
    const D: [u32; 6] = [
        0x43840000, 0x492ff500, 0x4de6f190, 0x51c52dac, 0x54c31fdd, 0x564ce175,
    ];
    let mut n = x + f32::from_bits(N[0]);
    for c in &N[1..] {
        n = n * x + f32::from_bits(*c);
    }
    let z = x * x;
    let mut d = z * f32::from_bits(D[0]) + f32::from_bits(D[1]);
    for c in &D[2..] {
        d = d * z + f32::from_bits(*c);
    }
    // Match the explicit comparison clamp, including NaN -> 0.
    let q = n / d;
    let q = if q > 0.0 { q } else { 0.0 };
    if q < 1.0 { q } else { 1.0 }
}

#[derive(Debug, Clone, Copy)]
pub enum Activation {
    Identity,
    Relu,
    Tanh,
    Sigmoid,
    Silu,
    LeakyRelu(f32),
}
impl Activation {
    pub fn apply(self, x: f32) -> f32 {
        match self {
            Self::Identity => x,
            Self::Relu => {
                if x > 0.0 {
                    x
                } else {
                    0.0
                }
            }
            Self::Tanh => x.tanh(),
            Self::Sigmoid => sigmoid(x),
            Self::Silu => x * sigmoid(x),
            Self::LeakyRelu(s) => {
                if x > 0.0 {
                    x
                } else {
                    x * s
                }
            }
        }
    }
}
