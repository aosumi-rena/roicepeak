use super::{FORMAT_NAME, MAX_BYTES, Payload};
use clipboard_win::{Clipboard, options::NoClear, raw};

const MAGIC: &[u8; 4] = b"RPC1";

fn format() -> Result<u32, String> {
    raw::register_format(FORMAT_NAME)
        .map(|id| id.get())
        .ok_or_else(|| "Cannot register the sentence clipboard format".into())
}

pub(crate) fn write(_: &eframe::egui::Context, payload: &Payload) -> Result<(), String> {
    let encoded = payload.encode()?;
    let text = payload.text();
    let mut frame = Vec::with_capacity(encoded.len() + 8);
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
    frame.extend_from_slice(&encoded);
    let format = format()?;
    let _clipboard = Clipboard::new().map_err(|error| format!("Cannot open clipboard: {error}"))?;
    raw::empty().map_err(|error| format!("Cannot clear clipboard: {error}"))?;
    raw::set_string_with(&text, NoClear).map_err(|error| format!("Cannot copy text: {error}"))?;
    raw::set_without_clear(format, &frame)
        .map_err(|error| format!("Cannot copy sentence edits: {error}"))
}

pub(crate) fn read() -> Result<Option<Payload>, String> {
    let format = format()?;
    let _clipboard = Clipboard::new().map_err(|error| format!("Cannot open clipboard: {error}"))?;
    if !raw::is_format_avail(format) {
        return Ok(None);
    }
    let size = raw::size(format)
        .ok_or("Cannot measure sentence clipboard data")?
        .get();
    if !(8..=MAX_BYTES + 4096).contains(&size) {
        return Err("Sentence clipboard allocation exceeds its limit".into());
    }
    let mut frame = Vec::new();
    raw::get_vec(format, &mut frame)
        .map_err(|error| format!("Cannot read sentence clipboard data: {error}"))?;
    if frame.len() != size || frame.get(..4) != Some(MAGIC.as_slice()) {
        return Err("Invalid sentence clipboard header".into());
    }
    let length = u32::from_le_bytes(frame[4..8].try_into().unwrap()) as usize;
    if length > MAX_BYTES || length > frame.len() - 8 {
        return Err("Invalid sentence clipboard length".into());
    }
    Payload::decode(&frame[8..8 + length]).map(Some)
}
