//! Declarative speaker factory metadata.
use super::{Description, required, text};
use roicepeak::vocoder::Effects;

pub(super) fn is_protocol3(description: &Description) -> Result<bool, String> {
    let entrance = &description.entrance;
    Ok(required(entrance, "code", 64)? == b"speaker_unified_v3"
        && required(entrance, "protocol_version", 4)? == [3, 0, 0, 0])
}

pub(super) fn vocoder_effects(description: &Description) -> Result<Effects, String> {
    if !is_protocol3(description)? {
        return Ok(Effects::default());
    }
    let entrance = &description.entrance;
    if required(entrance, "stylebook_tension_start", 4)?.len() != 4 {
        return Err("Unified stylebook_tension_start must contain four bytes".into());
    }
    let table = |key| super::inline(entrance, key, 4096 * 8).map(|bytes| bytes.unwrap_or_default());
    Effects::from_le_bytes(&table("posteffect_eq_p")?, &table("posteffect_eq_ap")?)
        .map_err(|error| error.to_string())
}

pub(super) fn speaker_identity(
    description: &Description,
    has_vector: bool,
) -> Result<usize, String> {
    let entrance = &description.entrance;
    // Treat factory identifiers as data and require the declared schema.
    let identifier = required(entrance, "code", 64)?;
    match identifier.as_slice() {
        b"speaker" | b"speaker_v2" => {
            let descriptor = required(entrance, "desc", 65_536)?;
            let word = descriptor
                .get(..2)
                .ok_or("Narrator speaker descriptor is truncated")?;
            Ok(usize::from(u16::from_le_bytes([word[0], word[1]])))
        }
        b"speaker_unified" => {
            // The model validates the nonnegative index against its embedding table.
            let descriptor = required(entrance, "speaker_id", 4)?;
            let word: [u8; 4] = descriptor
                .try_into()
                .map_err(|_| "Unified narrator speaker_id must contain four bytes")?;
            usize::try_from(i32::from_le_bytes(word))
                .map_err(|_| "Unified narrator speaker_id must be nonnegative".into())
        }
        b"speaker_unified_v2" if has_vector => {
            // The declared vector replaces speaker-table lookup.
            Ok(0)
        }
        b"speaker_unified_v2" => Err("Unified-v2 narrator requires its speaker vector".into()),
        b"speaker_unified_v3" => {
            let bytes: [u8; 4] = required(entrance, "protocol_version", 4)?
                .try_into()
                .map_err(|_| "Unified-v3 protocol_version must contain four bytes")?;
            match u32::from_le_bytes(bytes) {
                2 | 3 => {}
                _ => return Err("Unsupported unified-v3 protocol_version".into()),
            }
            if !has_vector {
                return Err("Unified-v3 narrator requires its speaker vector".into());
            }
            for (key, declared) in [
                ("compid_ms", description.pack_names.tts.as_deref()),
                ("compid_vo", description.pack_names.vo.as_deref()),
            ] {
                let bytes = required(entrance, key, 4096)?;
                let name = std::str::from_utf8(&bytes)
                    .map_err(|_| "Unified component identity must be UTF-8")?;
                text(name)?;
                if Some(name) != declared {
                    return Err(format!(
                        "Unified component {key} differs from its pack declaration"
                    ));
                }
            }
            for key in [
                "stylebook_bap_offset_start",
                "stylebook_volume_start",
                "stylebook_int_offset_start",
                "stylebook_overall_speed_start",
            ] {
                if required(entrance, key, 4)?.len() != 4 {
                    return Err(format!("Unified {key} must contain four bytes"));
                }
            }
            let phonemes = required(entrance, "stylebook_phoneme_list", 65_536)?;
            std::str::from_utf8(&phonemes).map_err(|_| "Unified style phonemes must be UTF-8")?;
            let languages = required(entrance, "lang_list", 65_536)?;
            let languages = std::str::from_utf8(&languages)
                .map_err(|_| "Unified language list must be UTF-8")?;
            if languages
                .split('\0')
                .filter(|name| !name.is_empty())
                .count()
                == 0
            {
                return Err("Unified language list is empty".into());
            }
            // The model loader validates the vector length.
            Ok(0)
        }
        _ => Err("Unsupported narrator code identifier/revision".into()),
    }
}
