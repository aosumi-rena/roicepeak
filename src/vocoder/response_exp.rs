//! Rounded endpoint exponential for packed filter responses.

const FRACTIONS: [f64; 64] = [
    f64::from_bits(0x3ff0_0000_0000_0000),
    f64::from_bits(0x3ff0_2c9a_3e77_8061),
    f64::from_bits(0x3ff0_59b0_d315_8574),
    f64::from_bits(0x3ff0_8745_1875_9bc8),
    f64::from_bits(0x3ff0_b558_6cf9_890f),
    f64::from_bits(0x3ff0_e3ec_32d3_d1a2),
    f64::from_bits(0x3ff1_1301_d012_5b51),
    f64::from_bits(0x3ff1_429a_aea9_2de0),
    f64::from_bits(0x3ff1_72b8_3c7d_517b),
    f64::from_bits(0x3ff1_a35b_eb6f_cb75),
    f64::from_bits(0x3ff1_d487_3168_b9aa),
    f64::from_bits(0x3ff2_063b_8862_8cd6),
    f64::from_bits(0x3ff2_387a_6e75_6238),
    f64::from_bits(0x3ff2_6b45_65e2_7cdd),
    f64::from_bits(0x3ff2_9e9d_f51f_dee1),
    f64::from_bits(0x3ff2_d285_a6e4_030b),
    f64::from_bits(0x3ff3_06fe_0a31_b715),
    f64::from_bits(0x3ff3_3c08_b264_16ff),
    f64::from_bits(0x3ff3_71a7_373a_a9cb),
    f64::from_bits(0x3ff3_a7db_34e5_9ff7),
    f64::from_bits(0x3ff3_dea6_4c12_3422),
    f64::from_bits(0x3ff4_160a_21f7_2e2a),
    f64::from_bits(0x3ff4_4e08_6061_892d),
    f64::from_bits(0x3ff4_86a2_b5c1_3cd0),
    f64::from_bits(0x3ff4_bfda_d536_2a27),
    f64::from_bits(0x3ff4_f9b2_769d_2ca7),
    f64::from_bits(0x3ff5_342b_569d_4f82),
    f64::from_bits(0x3ff5_6f47_36b5_27da),
    f64::from_bits(0x3ff5_ab07_dd48_5429),
    f64::from_bits(0x3ff5_e76f_15ad_2148),
    f64::from_bits(0x3ff6_247e_b03a_5585),
    f64::from_bits(0x3ff6_6238_8255_2225),
    f64::from_bits(0x3ff6_a09e_667f_3bcd),
    f64::from_bits(0x3ff6_dfb2_3c65_1a2f),
    f64::from_bits(0x3ff7_1f75_e8ec_5f74),
    f64::from_bits(0x3ff7_5feb_5642_67c9),
    f64::from_bits(0x3ff7_a114_73eb_0187),
    f64::from_bits(0x3ff7_e2f3_36cf_4e62),
    f64::from_bits(0x3ff8_2589_994c_ce13),
    f64::from_bits(0x3ff8_68d9_9b44_92ed),
    f64::from_bits(0x3ff8_ace5_422a_a0db),
    f64::from_bits(0x3ff8_f1ae_9915_7736),
    f64::from_bits(0x3ff9_3737_b0cd_c5e5),
    f64::from_bits(0x3ff9_7d82_9fde_4e50),
    f64::from_bits(0x3ff9_c491_82a3_f090),
    f64::from_bits(0x3ffa_0c66_7b5d_e565),
    f64::from_bits(0x3ffa_5503_b23e_255d),
    f64::from_bits(0x3ffa_9e6b_5579_fdbf),
    f64::from_bits(0x3ffa_e89f_995a_d3ad),
    f64::from_bits(0x3ffb_33a2_b84f_15fb),
    f64::from_bits(0x3ffb_7f76_f2fb_5e47),
    f64::from_bits(0x3ffb_cc1e_904b_c1d2),
    f64::from_bits(0x3ffc_199b_dd85_529c),
    f64::from_bits(0x3ffc_67f1_2e57_d14b),
    f64::from_bits(0x3ffc_b720_dcef_9069),
    f64::from_bits(0x3ffd_072d_4a07_897c),
    f64::from_bits(0x3ffd_5818_dcfb_a487),
    f64::from_bits(0x3ffd_a9e6_03db_3285),
    f64::from_bits(0x3ffd_fc97_337b_9b5f),
    f64::from_bits(0x3ffe_502e_e78b_3ff6),
    f64::from_bits(0x3ffe_a4af_a2a4_90da),
    f64::from_bits(0x3ffe_fa1b_ee61_5a27),
    f64::from_bits(0x3fff_5076_5b6e_4540),
    f64::from_bits(0x3fff_a7c1_819e_90d8),
];

pub(super) fn exponential(value: f32, fused: bool) -> f32 {
    debug_assert!(value.is_finite());
    let x = f64::from(value);
    let power = x * f64::from_bits(0x4057_1547_652b_82fe);
    if power >= 8192.0 {
        return f32::INFINITY;
    }
    if power < -9600.0 {
        return 0.0;
    }
    let n = power.round_ties_even() as i32;
    let index = (n & 63) as usize;
    let exponent = (n - index as i32) / 64;
    let step = f64::from_bits(0x3f86_2e42_fefa_39ef);
    let r = if fused {
        (-f64::from(n)).mul_add(step, x)
    } else {
        x - f64::from(n) * step
    };
    let square = r * r;
    let fraction = if fused {
        let polynomial = (1.0f64 / 6.0).mul_add(r, 0.5);
        let residual = square.mul_add(polynomial, r);
        residual.mul_add(FRACTIONS[index], FRACTIONS[index])
    } else {
        let polynomial = r * (1.0 / 6.0) + 0.5;
        let residual = square * polynomial + r;
        residual * FRACTIONS[index] + FRACTIONS[index]
    };
    let scale = f64::from_bits(((exponent + 1023) as u64) << 52);
    (fraction * scale) as f32
}
