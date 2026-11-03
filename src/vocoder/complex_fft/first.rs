const H: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn roots(length: usize, twisted: bool) -> (f32, f32) {
    // Rounded sin/cos of the quantized angle used by each fixed-size stage.
    let (c, s) = match (length, twisted) {
        (128, false) => (0x3f7ec46d, 0x3dc8bd36),
        (512, false) => (0x3f7fec43, 0x3cc90ab0),
        (2048, false) => (0x3f7ffec4, 0x3bc90f88),
        (128, true) => (0x3f7fb10f, 0x3d48fb30),
        (512, true) => (0x3f7ffb11, 0x3c490e90),
        _ => unreachable!("fixed complex FFT stage length"),
    };
    (f32::from_bits(c), f32::from_bits(s))
}

fn ordinary_butterfly(
    data: &mut [f32],
    j: usize,
    q: usize,
    weights: Option<[f32; 4]>,
    center: bool,
    inverse: bool,
) {
    let a = [data[j], data[j + 1]];
    let b = [data[j + q], data[j + q + 1]];
    let c = [data[j + 2 * q], data[j + 2 * q + 1]];
    let d = [data[j + 3 * q], data[j + 3 * q + 1]];
    let sum02 = [c[0] + a[0], c[1] + a[1]];
    let sum13 = [d[0] + b[0], d[1] + b[1]];
    let dr02 = a[0] - c[0];
    let dr13 = b[0] - d[0];
    let di13 = b[1] - d[1];
    data[j] = sum13[0] + sum02[0];
    data[j + q] = sum02[0] - sum13[0];
    let (ur, ui, vr, vi) = if inverse {
        data[j + 1] = -(sum13[1] + sum02[1]);
        data[j + q + 1] = sum13[1] - sum02[1];
        let di02 = c[1] - a[1];
        (di13 + dr02, dr13 + di02, dr02 - di13, di02 - dr13)
    } else {
        data[j + 1] = sum13[1] + sum02[1];
        data[j + q + 1] = sum02[1] - sum13[1];
        let di02 = a[1] - c[1];
        (dr02 - di13, dr13 + di02, di13 + dr02, di02 - dr13)
    };
    let (out2, out3) = if let Some([c, s, c3, ns3]) = weights {
        if center {
            (
                [(ur - ui) * H, (ui + ur) * H],
                [(vr + vi) * -H, (vi - vr) * -H],
            )
        } else {
            (
                [ur * c - ui * s, ur * s + ui * c],
                [vi * ns3 + vr * c3, vi * c3 - vr * ns3],
            )
        }
    } else {
        ([ur, ui], [vr, vi])
    };
    data[j + 2 * q..j + 2 * q + 2].copy_from_slice(&out2);
    data[j + 3 * q..j + 3 * q + 2].copy_from_slice(&out3);
}

pub(super) fn ordinary(data: &mut [f32], inverse: bool) {
    let q = data.len() / 4;
    let m = q / 2;
    ordinary_butterfly(data, 0, q, None, false, inverse);
    let (c, s) = roots(data.len(), false);
    let four_sc = (4.0 * s) * c;
    let mut co = [c - four_sc * s, c];
    let mut so = [s - four_sc * c, s];
    let mut ce = [1.0; 2];
    let mut se = [0.0; 2];
    let step = [so[0] + so[0], so[1] + so[1]];
    for j in (2..m - 2).step_by(4) {
        if data.len() == 2048 && j == 130 {
            let bc = f32::from_bits(0x3f6c835e);
            let bs = f32::from_bits(0x3ec3ef16);
            let factor = (4.0f32 * bs) * bc;
            ce = [bc - factor * bs, bc];
            se = [bs - factor * bc, bs];
            let real_left = bc * c;
            let real_right = bs * s;
            let nc = real_left - real_right;
            let imaginary_left = bc * s;
            let imaginary_right = bs * c;
            let ns = imaginary_left + imaginary_right;
            let factor = (4.0f32 * ns) * nc;
            co = [nc - factor * ns, nc];
            so = [ns - factor * nc, ns];
        }
        let old_co = co;
        let old_so = so;
        for k in 0..2 {
            se[k] += co[k] * step[k];
        }
        ordinary_butterfly(
            data,
            j,
            q,
            Some([co[1], so[1], co[0], so[0]]),
            false,
            inverse,
        );
        for k in 0..2 {
            ce[k] -= so[k] * step[k];
        }
        ordinary_butterfly(
            data,
            j + 2,
            q,
            Some([ce[1], se[1], ce[0], se[0]]),
            false,
            inverse,
        );
        ordinary_butterfly(
            data,
            q - j,
            q,
            Some([old_so[1], old_co[1], old_so[0], old_co[0]]),
            false,
            inverse,
        );
        for k in 0..2 {
            co[k] -= se[k] * step[k];
        }
        ordinary_butterfly(
            data,
            q - j - 2,
            q,
            Some([se[1], ce[1], se[0], ce[0]]),
            false,
            inverse,
        );
        for k in 0..2 {
            so[k] = ce[k] * step[k] + old_so[k];
        }
    }
    ordinary_butterfly(
        data,
        m - 2,
        q,
        Some([co[1], so[1], co[0], so[0]]),
        false,
        inverse,
    );
    ordinary_butterfly(data, m, q, Some([H, H, -H, -H]), true, inverse);
    ordinary_butterfly(
        data,
        m + 2,
        q,
        Some([so[1], co[1], so[0], co[0]]),
        false,
        inverse,
    );
}

fn twisted_butterfly(
    data: &mut [f32],
    j: usize,
    q: usize,
    weights: Option<[f32; 8]>,
    center: bool,
) {
    let a = [data[j], data[j + 1]];
    let b = [data[j + q], data[j + q + 1]];
    let c = [data[j + 2 * q], data[j + 2 * q + 1]];
    let d = [data[j + 3 * q], data[j + 3 * q + 1]];
    let (ur, ui) = (a[0] - c[1], c[0] + a[1]);
    let (vr, vi) = (b[0] - d[1], d[0] + b[1]);
    let (wr, wi) = (c[1] + a[0], a[1] - c[0]);
    let (tr, ti) = (d[1] + b[0], b[1] - d[0]);
    let (out0, out1, out2, out3) = if let Some([cu, su, cv, sv, cw, nsw, ct, nst]) = weights {
        let (urr, uii) = (ur * cu - ui * su, ur * su + ui * cu);
        let (vrr, vii) = (vr * cv - vi * sv, vr * sv + vi * cv);
        let (out2, out3) = if center {
            let (wrr, wii) = (wr * cw - wi * -nsw, wr * -nsw + wi * cw);
            let (trr, tii) = (tr * -ct - ti * nst, tr * nst + ti * -ct);
            ([wrr - trr, wii - tii], [trr + wrr, tii + wii])
        } else {
            let (wrr, wii) = (wi * nsw + wr * cw, wi * cw - wr * nsw);
            let (trr, tii) = (ti * nst + tr * ct, ti * ct - tr * nst);
            ([trr + wrr, tii + wii], [wrr - trr, wii - tii])
        };
        ([vrr + urr, vii + uii], [urr - vrr, uii - vii], out2, out3)
    } else {
        let (vrr, vii) = ((vr - vi) * H, (vi + vr) * H);
        let (trr, tii) = ((ti + tr) * H, (tr - ti) * H);
        (
            [vrr + ur, vii + ui],
            [ur - vrr, ui - vii],
            [wr - trr, tii + wi],
            [trr + wr, wi - tii],
        )
    };
    for (k, value) in [out0, out1, out2, out3].iter().enumerate() {
        data[j + k * q..j + k * q + 2].copy_from_slice(value);
    }
}

pub(super) fn twisted(data: &mut [f32]) {
    let q = data.len() / 4;
    let m = q / 2;
    twisted_butterfly(data, 0, q, None, false);
    let (c, s) = roots(data.len(), true);
    let four_sc = (4.0 * s) * c;
    let mut co = [c, c - four_sc * s];
    let mut so = [s, s - four_sc * c];
    let mut ce = [1.0; 2];
    let mut se = [0.0; 2];
    let step = [so[0] + so[0], so[1] + so[1]];
    let mut side = [
        (so[0] + co[0]) * H,
        (so[1] + co[1]) * -H,
        (co[0] - so[0]) * H,
        (co[1] - so[1]) * -H,
    ];
    let mut base = [H, -H, H, -H];
    for j in (2..m - 2).step_by(4) {
        let old_co = co;
        let old_so = so;
        twisted_butterfly(
            data,
            j,
            q,
            Some([
                co[0], so[0], side[2], side[0], co[1], so[1], side[3], side[1],
            ]),
            false,
        );
        for k in 0..2 {
            se[k] += co[k] * step[k];
        }
        for k in 0..2 {
            co[k] -= se[k] * step[k];
        }
        for k in 0..2 {
            ce[k] -= so[k] * step[k];
        }
        let minus: [f32; 4] = std::array::from_fn(|k| base[k] - side[k] * step[k % 2]);
        let plus: [f32; 4] = std::array::from_fn(|k| base[k] + side[k] * step[k % 2]);
        let even = [minus[0], minus[1], plus[2], plus[3]];
        twisted_butterfly(
            data,
            j + 2,
            q,
            Some([
                ce[0], se[0], even[0], even[2], ce[1], se[1], even[1], even[3],
            ]),
            false,
        );
        twisted_butterfly(
            data,
            q - j,
            q,
            Some([
                side[0], side[2], old_so[0], old_co[0], side[1], side[3], old_so[1], old_co[1],
            ]),
            false,
        );
        for k in 0..2 {
            so[k] = ce[k] * step[k] + old_so[k];
        }
        twisted_butterfly(
            data,
            q - j - 2,
            q,
            Some([
                even[2], even[0], se[0], ce[0], even[3], even[1], se[1], ce[1],
            ]),
            false,
        );
        let change: [f32; 4] = std::array::from_fn(|k| even[k] * step[k % 2]);
        side = [
            side[0] + change[0],
            side[1] + change[1],
            side[2] - change[2],
            side[3] - change[3],
        ];
        base = even;
    }
    twisted_butterfly(
        data,
        m - 2,
        q,
        Some([
            co[0], so[0], side[2], side[0], co[1], so[1], side[3], side[1],
        ]),
        false,
    );
    let (c, s) = (f32::from_bits(0x3f6c835e), f32::from_bits(0x3ec3ef15));
    twisted_butterfly(data, m, q, Some([c, s, s, c, s, -c, -c, s]), true);
    twisted_butterfly(
        data,
        m + 2,
        q,
        Some([
            side[0], side[2], so[0], co[0], side[1], side[3], so[1], co[1],
        ]),
        false,
    );
}
