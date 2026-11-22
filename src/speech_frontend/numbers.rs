pub(super) struct Number {
    pub(super) end: usize,
    pub(super) sign: Option<Sign>,
    pub(super) integer: String,
    pub(super) fraction: String,
    pub(super) percent: bool,
}

#[derive(Clone, Copy)]
pub(super) enum Sign {
    Plus,
    Minus,
}

pub(super) fn sign(ch: char) -> Option<Sign> {
    match ch {
        '+' | '＋' => Some(Sign::Plus),
        '-' | '－' | '−' => Some(Sign::Minus),
        _ => None,
    }
}

pub(super) fn begins(ch: char) -> bool {
    digit(ch).is_some() || sign(ch).is_some() || matches!(ch, '.' | '．')
}

fn digit(ch: char) -> Option<u8> {
    match ch {
        '0'..='9' => Some(ch as u8 - b'0'),
        '０'..='９' => Some((u32::from(ch) - u32::from('０')) as u8),
        _ => None,
    }
}

fn digits(text: &str, end: &mut usize) -> String {
    let mut value = String::new();
    for ch in text[*end..].chars() {
        let Some(number) = digit(ch) else { break };
        value.push(char::from(b'0' + number));
        *end += ch.len_utf8();
    }
    value
}

pub(super) fn scan(source: &str, start: usize) -> Option<Number> {
    let text = &source[start..];
    let first = text.chars().next()?;
    if !begins(first) {
        return None;
    }
    let sign = sign(first);
    if digit(first).is_none()
        && source[..start].chars().next_back().is_some_and(|ch| {
            ch.is_ascii_alphanumeric()
                || digit(ch).is_some()
                || matches!(ch, '_' | 'Ａ'..='Ｚ' | 'ａ'..='ｚ')
        })
    {
        return None;
    }
    let mut end = 0;
    if sign.is_some() {
        end = first.len_utf8();
    }
    let mut integer = digits(text, &mut end);
    if !integer.is_empty() && integer.len() <= 3 && !integer.starts_with('0') {
        while let Some(separator @ (',' | '，')) = text[end..].chars().next() {
            let mut stop = end + separator.len_utf8();
            let full_width_digit = |ch| matches!(ch, '０'..='９');
            if separator == '，'
                && !(text[..end].ends_with(full_width_digit)
                    && text[stop..].starts_with(full_width_digit))
            {
                break;
            }
            let group = digits(text, &mut stop);
            if group.len() != 3 {
                break;
            }
            integer.push_str(&group);
            end = stop;
        }
    }
    let mut fraction = String::new();
    if let Some(separator @ ('.' | '．')) = text[end..].chars().next() {
        let mut stop = end + separator.len_utf8();
        fraction = digits(text, &mut stop);
        if !fraction.is_empty() {
            end = stop;
        }
    }
    if integer.is_empty() {
        if fraction.is_empty() {
            return None;
        }
        integer.push('0');
    }
    let percent = if let Some(sign @ ('%' | '％')) = text[end..].chars().next() {
        end += sign.len_utf8();
        true
    } else {
        false
    };
    Some(Number {
        end,
        sign,
        integer,
        fraction,
        percent,
    })
}
