pub(super) const S: [f64; 6] = [
    f64::from_bits(0xbfc5_5555_5555_5555),
    f64::from_bits(0x3f81_1111_1111_0bb3),
    f64::from_bits(0xbf2a_01a0_19e8_3e5c),
    f64::from_bits(0x3ec7_1de3_796c_de01),
    f64::from_bits(0xbe5a_e600_b42f_dfa7),
    f64::from_bits(0x3de5_e0b2_f9a4_3bb8),
];
const C: [f64; 6] = [
    f64::from_bits(0x3fa5_5555_5555_5555),
    f64::from_bits(0xbf56_c16c_16c1_6967),
    f64::from_bits(0x3efa_01a0_19f4_ec91),
    f64::from_bits(0xbe92_7e4f_a17f_667b),
    f64::from_bits(0x3e21_eeb6_9038_2eec),
    f64::from_bits(0xbda9_07db_4725_8aa7),
];

fn horner(square: f64, coefficients: &[f64; 6]) -> f64 {
    let mut value = square.mul_add(coefficients[5], coefficients[4]);
    for i in (0..4).rev() {
        value = value.mul_add(square, coefficients[i]);
    }
    value
}

fn grouped(square: f64, coefficients: &[f64; 6]) -> f64 {
    let cube = (square * square) * square;
    let high = (coefficients[5] * square + coefficients[4]) * square + coefficients[3];
    let low = (coefficients[2] * square + coefficients[1]) * square + coefficients[0];
    high * cube + low
}

pub(super) fn small_sine(x: f64, fused: bool) -> f64 {
    let square = x * x;
    let cube = x * square;
    if fused {
        cube.mul_add(horner(square, &S), x)
    } else {
        x + cube * grouped(square, &S)
    }
}

pub(super) fn small_cosine(x: f64, fused: bool) -> f64 {
    let square = x * x;
    if fused {
        horner(square, &C)
            .mul_add(square, -0.5)
            .mul_add(square, 1.0)
    } else {
        let fourth = square * square;
        let eighth = fourth * fourth;
        let twelfth = fourth * eighth;
        let a = (C[1] * square + C[0]) * fourth;
        let b = (C[3] * square + C[2]) * eighth;
        let c = (C[5] * square + C[4]) * twelfth;
        let half = square * -0.5;
        let upper = half + 1.0;
        let error = (1.0 - upper) + half;
        (error + ((a + b) + c)) + upper
    }
}

pub(super) fn reduced_sine(r: f64, tail: f64, fused: bool) -> f64 {
    let square = r * r;
    let cube = r * square;
    if fused {
        let mut p = square.mul_add(S[5], S[4]);
        for i in (1..4).rev() {
            p = p.mul_add(square, S[i]);
        }
        let a = cube * p;
        let b = tail * 0.5;
        let a = b - a;
        let a = square * a;
        let a = a - tail;
        let a = (-cube).mul_add(S[0], a);
        r - a
    } else {
        let a = cube * grouped(square, &S) - (square * 0.5) * tail;
        (tail + a) + r
    }
}

pub(super) fn reduced_cosine(r: f64, tail: f64, fused: bool) -> f64 {
    let square = r * r;
    let half = square * 0.5;
    if fused {
        let upper = 1.0 - half;
        let error = (1.0 - upper) - half;
        let error = (-r).mul_add(tail, error);
        let p = horner(square, &C).mul_add(square * square, error);
        p + upper
    } else {
        let saved = tail * r;
        let compensation = ((half - 1.0) + 1.0) - half;
        let compensation = compensation - saved;
        let p = grouped(square, &C) * (square * square);
        (p + compensation) - (half - 1.0)
    }
}
