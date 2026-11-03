pub(super) fn parse(text: &str, previous: f64, absolute: bool) -> f64 {
    let text = text.trim();
    let (sign, text) = text.strip_prefix('-').map_or((1.0, text), |s| (-1.0, s));
    let fields: Vec<_> = if text.is_empty() {
        Vec::new()
    } else {
        text.split(['.', ':']).collect()
    };
    let decimal = |part: &str| {
        let mut end = 0;
        for (i, byte) in part.bytes().enumerate() {
            if byte.is_ascii_digit() || (i == 0 && matches!(byte, b'+' | b'-')) {
                end = i + 1;
            } else {
                break;
            }
        }
        part[..end].parse::<f64>().unwrap_or(0.0)
    };
    let fraction = |part: &str| decimal(part) / 10.0f64.powi(part.len() as i32);
    let value = match fields.as_slice() {
        [a] => decimal(a),
        [a, b] => decimal(a) + fraction(b),
        [a, b, c] => decimal(a) * 60.0 + decimal(b) + fraction(c),
        [a, b, c, d] => decimal(b) * 60.0 + decimal(a) * 3600.0 + decimal(c) + fraction(d),
        _ => return previous,
    } * sign;
    if !value.is_finite() {
        previous
    } else if absolute {
        value.max(0.0)
    } else {
        value
    }
}

pub(super) fn absolute(value: f64) -> String {
    let scaled = (value.abs() * 10.0 * 10.0).round();
    if !scaled.is_finite() || scaled >= u64::MAX as f64 {
        return String::new();
    }
    let ticks = scaled as u64;
    let (centis, seconds, minutes, hours) = (
        ticks % 100,
        ticks / 100 % 60,
        ticks / 6000 % 60,
        ticks / 360000,
    );
    let sign = if value.is_sign_negative() && ticks != 0 {
        "-"
    } else {
        ""
    };
    if hours == 0 {
        format!("{sign}{minutes:02}.{seconds:02}.{centis:02}")
    } else {
        format!("{sign}{hours}.{minutes:02}.{seconds:02}.{centis:02}")
    }
}

pub(super) fn relative(value: f64) -> String {
    let scientific = format!("{value:.5e}");
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return scientific;
    };
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let trim = |mut s: String| {
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        s
    };
    if (-4..6).contains(&exponent) {
        trim(format!("{value:.*}", (5 - exponent).max(0) as usize))
    } else {
        format!(
            "{}e{}{:02}",
            trim(mantissa.to_owned()),
            if exponent >= 0 { "+" } else { "-" },
            exponent.unsigned_abs()
        )
    }
}
