use super::check;

pub(super) fn decode(bytes: &[u8]) -> Result<String, String> {
    let text = if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe, 0x00, 0x00]) {
        utf32(bytes, true)?
    } else if let Some(bytes) = bytes.strip_prefix(&[0x00, 0x00, 0xfe, 0xff]) {
        utf32(bytes, false)?
    } else if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        utf16(bytes, true)?
    } else if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        utf16(bytes, false)?
    } else {
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        std::str::from_utf8(bytes)
            .map_err(
                |_| "SRT encoding is not recognized; use UTF-8 or Unicode with a byte-order mark",
            )?
            .to_owned()
    };
    check(&text)?;
    Ok(text)
}

fn utf16(bytes: &[u8], little: bool) -> Result<String, String> {
    if !bytes.len().is_multiple_of(2) {
        return Err("Incomplete UTF-16 SRT character".into());
    }
    let words = bytes
        .chunks_exact(2)
        .map(|pair| {
            let pair = [pair[0], pair[1]];
            if little {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        })
        .collect::<Vec<_>>();
    String::from_utf16(&words).map_err(|_| "Invalid UTF-16 SRT character".into())
}

fn utf32(bytes: &[u8], little: bool) -> Result<String, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err("Incomplete UTF-32 SRT character".into());
    }
    bytes
        .chunks_exact(4)
        .map(|quad| {
            let quad = [quad[0], quad[1], quad[2], quad[3]];
            let word = if little {
                u32::from_le_bytes(quad)
            } else {
                u32::from_be_bytes(quad)
            };
            char::from_u32(word).ok_or_else(|| "Invalid UTF-32 SRT character".to_owned())
        })
        .collect()
}
