fn pair(data: &mut [f32; 1024], j: usize, wr: f32, wi: f32, inverse: bool) {
    let (ar, ai, br, bi) = (data[j], data[j + 1], data[1024 - j], data[1025 - j]);
    let dr = ar - br;
    let si = bi + ai;
    let (yr, yi) = if inverse {
        (si * wi + dr * wr, si * wr - dr * wi)
    } else {
        (dr * wr - si * wi, dr * wi + si * wr)
    };
    data[j] = ar - yr;
    data[j + 1] = ai - yi;
    data[1024 - j] = yr + br;
    data[1025 - j] = bi - yi;
}

pub(super) fn apply(data: &mut [f32; 1024], inverse: bool) {
    // Rounded roots for the quantized pi/1024 angle.
    let c = f32::from_bits(0x3f7fffb1);
    let s = f32::from_bits(0x3b490fc6);
    let mut iodd = c * s;
    let mut rodd = s * s;
    let cstep = 1.0 - (rodd + rodd);
    let sstep = iodd + iodd;
    let step = 4.0 * iodd;
    let mut reven = 0.0;
    let mut ieven = 0.0;
    let mut position = 512;
    loop {
        let bound = (position - 256usize).max(4);
        let mut j = position - 2;
        while j > bound {
            pair(data, j, rodd, iodd, inverse);
            reven += iodd * step;
            let next_iodd = ((0.5 - f64::from(reven)) * f64::from(step) + f64::from(iodd)) as f32;
            let next_ieven = ((0.5 - f64::from(rodd)) * f64::from(step) + f64::from(ieven)) as f32;
            pair(data, j - 2, reven, next_ieven, inverse);
            rodd = next_ieven * step + rodd;
            iodd = next_iodd;
            ieven = next_ieven;
            j -= 4;
        }
        if bound == 4 {
            break;
        }
        // This fixed transform refreshes once, at the quantized pi/4 angle.
        let half_s = f32::from_bits(0x3eb504f3);
        let half_c = f32::from_bits(0x3eb504f3);
        rodd = (sstep * half_c + 0.5) - cstep * half_s;
        iodd = cstep * half_c + sstep * half_s;
        reven = 0.5 - half_s;
        ieven = half_c;
        position = bound;
    }
    pair(data, 2, rodd, iodd, inverse);
}
