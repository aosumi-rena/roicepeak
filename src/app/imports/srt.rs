use super::{MAX_BLOCKS, MAX_TEXT, check, line_error};

#[derive(serde::Serialize)]
pub struct Cue {
    pub index: i32,
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub source_line: usize,
}

#[derive(serde::Serialize)]
pub struct Captions {
    pub cues: Vec<Cue>,
    pub error: Option<String>,
}

pub(super) fn parse(text: &str) -> Result<Captions, String> {
    check(text)?;
    let mut result = Captions {
        cues: Vec::new(),
        error: None,
    };
    if text.is_empty() {
        result.error = Some("Failed to load SRT file".into());
        return Ok(result);
    }
    let lines = lines(text)?;
    let mut cursor = 0;
    while cursor < lines.len() {
        let number = lines[cursor].trim();
        if number.is_empty() {
            cursor += 1;
            continue;
        }
        let source_line = cursor + 1;
        let Some(index) = integer(number) else {
            result.error = Some(line_error(source_line, "invalid SRT cue index"));
            break;
        };
        cursor += 1;
        let Some((start, end)) = range(lines.get(cursor).copied().unwrap_or("")) else {
            result.error = Some(line_error(cursor + 1, "invalid SRT time range"));
            break;
        };
        cursor += 1;
        let first = cursor;
        let mut bytes = 0usize;
        while cursor < lines.len() && !lines[cursor].is_empty() {
            bytes = bytes
                .checked_add(lines[cursor].len() + usize::from(cursor > first))
                .ok_or("SRT text size overflow")?;
            if bytes > MAX_TEXT {
                return Err(line_error(source_line, "SRT cue text exceeds 1 MiB"));
            }
            cursor += 1;
        }
        if result.cues.len() >= MAX_BLOCKS {
            return Err(line_error(source_line, "more than 10000 SRT cues"));
        }
        result.cues.push(Cue {
            index,
            start,
            end,
            text: lines[first..cursor].join("\n"),
            source_line,
        });
    }
    Ok(result)
}

fn lines(text: &str) -> Result<Vec<&str>, String> {
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let mut start = 0;
    let mut cursor = 0;
    while cursor < bytes.len() {
        if matches!(bytes[cursor], b'\r' | b'\n') {
            if result.len() >= 100_000 {
                return Err("Import exceeds 100000 lines".into());
            }
            result.push(&text[start..cursor]);
            let crlf = bytes[cursor] == b'\r' && bytes.get(cursor + 1) == Some(&b'\n');
            cursor += if crlf { 2 } else { 1 };
            start = cursor;
        } else {
            cursor += 1;
        }
    }
    result.push(&text[start..]);
    Ok(result)
}

fn integer(value: &str) -> Option<i32> {
    if value.is_empty() {
        return Some(0);
    }
    let bytes = value.as_bytes();
    let mut cursor = 0;
    while bytes
        .get(cursor)
        .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c))
    {
        cursor += 1;
    }
    let negative = bytes.get(cursor) == Some(&b'-');
    if negative || bytes.get(cursor) == Some(&b'+') {
        cursor += 1;
    }
    let first = cursor;
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
    if cursor == first || cursor != bytes.len() {
        return None;
    }
    Some(if negative {
        (0u32.wrapping_sub(number)) as i32
    } else {
        number as i32
    })
}

fn range(line: &str) -> Option<(f64, f64)> {
    let (left, right) = line.split_once("-->")?;
    Some((clock(left.trim())?, clock(right.trim())?))
}

fn clock(value: &str) -> Option<f64> {
    let (left, right) = value.split_once(',')?;
    let left = left.trim();
    if left.is_empty() {
        return None;
    }
    let mut parts = left.split(':');
    let hours = integer(parts.next()?)?;
    let minutes = integer(parts.next()?)?;
    let seconds = integer(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    let milliseconds = integer(right.trim())?;
    let seconds = hours
        .wrapping_mul(60)
        .wrapping_add(minutes)
        .wrapping_mul(60)
        .wrapping_add(seconds);
    Some(f64::from(seconds) + f64::from(milliseconds) / 1000.0)
}
