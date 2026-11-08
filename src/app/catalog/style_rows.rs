use roicepeak::{
    dnni,
    stylebook::{Offsets, Stylebook},
};
use std::collections::BTreeMap;

pub(super) fn floats(value: &str, limit: usize) -> Result<Vec<u8>, String> {
    if value.is_empty() || value.len() % 8 != 0 || value.len() / 2 > limit {
        return Err("Voice hexadecimal data must contain bounded float32 values".into());
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err("Invalid voice hexadecimal data".to_owned()),
    };
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok((digit(pair[0])? << 4) | digit(pair[1])?))
        .collect::<Result<Vec<u8>, String>>()?;
    if bytes
        .chunks_exact(4)
        .any(|v| !f32::from_le_bytes(v.try_into().unwrap()).is_finite())
    {
        return Err("Voice hexadecimal data contains nonfinite values".into());
    }
    Ok(bytes)
}

pub(super) fn replace(
    bytes: &mut [u8],
    replacements: &BTreeMap<String, String>,
    offsets: Option<&Offsets>,
    phonemes: &[String],
    embedding_channels: Option<usize>,
) -> Result<(), String> {
    if replacements.is_empty() {
        return Ok(());
    }
    let patches = {
        let root = dnni::parse(bytes).map_err(|e| e.to_string())?;
        let style = match embedding_channels {
            Some(width) => {
                Stylebook::compile_for_model(&root, offsets.cloned(), phonemes.to_owned(), width)
            }
            None => Stylebook::compile(&root, offsets.cloned(), phonemes.to_owned()),
        }
        .map_err(|e| e.to_string())?;
        let width = style
            .mix(&vec![0.0; style.emotions.len()])
            .map_err(|e| e.to_string())?
            .embedding
            .len();
        let rows = if root.tag == "_stbkv0" {
            &root.children
        } else {
            &root.children[0].children
        };
        let mut patches = Vec::new();
        for (key, value) in replacements {
            let index = style
                .emotions
                .iter()
                .position(|e| e == key)
                .ok_or_else(|| format!("Unknown voice emotion {key:?}"))?;
            let values = floats(value, 4 * 1024 * 1024)?;
            if values.len() != width * 4 {
                return Err(format!(
                    "Voice emotion {key:?} requires {width} float32 values"
                ));
            }
            patches.push((rows[index + 1].offset + 24, values));
        }
        patches
    };
    for (start, values) in patches {
        bytes
            .get_mut(start..start + values.len())
            .ok_or("Voice style row is truncated")?
            .copy_from_slice(&values);
    }
    Ok(())
}
