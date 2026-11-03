//! The millisecond integer prefix used for the following speech block.
pub(super) fn delay(time: Option<&str>) -> f64 {
    let Some(time) = time.filter(|time| time.contains("ms")) else {
        return 0.0;
    };
    f64::from(integer(&time.replace("ms", ""))) / 1000.0
}

fn integer(text: &str) -> i32 {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while bytes
        .get(cursor)
        .is_some_and(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c))
    {
        cursor += 1;
    }
    let negative = bytes.get(cursor) == Some(&b'-');
    if negative || bytes.get(cursor) == Some(&b'+') {
        cursor += 1;
    }
    let limit = if negative {
        2_147_483_648u32
    } else {
        2_147_483_647u32
    };
    let mut number = 0u32;
    while let Some(&digit) = bytes.get(cursor).filter(|digit| digit.is_ascii_digit()) {
        number = number
            .saturating_mul(10)
            .saturating_add(u32::from(digit - b'0'))
            .min(limit);
        cursor += 1;
    }
    if negative {
        (0u32.wrapping_sub(number)) as i32
    } else {
        number as i32
    }
}
