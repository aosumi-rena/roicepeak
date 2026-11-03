use roicepeak::phonemes::Syllable;

pub(super) fn parse(text: &str) -> Result<(String, Vec<Syllable>), String> {
    if text.len() > 4096 || text.contains('\0') {
        return Err("Dictionary pronunciation exceeds its text limits".into());
    }
    let trimmed = trim(text)?;
    roicepeak::japanese::normalize_edit_reading(trimmed)
        .map_err(|_| "Please input a pronunciation using kanas.".into())
}

fn trim(text: &str) -> Result<&str, String> {
    let mut start = 0;
    for (at, c) in text.char_indices() {
        if !space(c)? {
            start = at;
            break;
        }
        start = at + c.len_utf8();
    }
    let rest = &text[start..];
    let mut end = rest.len();
    for (at, c) in rest.char_indices().rev() {
        if !space(c)? {
            break;
        }
        end = at;
    }
    Ok(&rest[..end])
}

#[cfg(windows)]
fn space(c: char) -> Result<bool, String> {
    Ok(
        matches!(c as u32 as u16, 0x0009..=0x000d | 0x0020 | 0x0085 | 0x00a0
        | 0x1680 | 0x180e | 0x2000..=0x200a | 0x2028..=0x2029 | 0x202f
        | 0x205f | 0x3000),
    )
}

#[cfg(not(windows))]
fn space(c: char) -> Result<bool, String> {
    if c.is_whitespace() && !c.is_ascii() {
        return Err("Dictionary Unicode whitespace classification requires Windows".into());
    }
    Ok(matches!(c, '\t'..='\r' | ' '))
}
