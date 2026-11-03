//! Mono PCM16 output; FLAC is a real lossless encoder over the same quantization.
use super::Format;
use flacenc::{component::BitRepr, error::Verify};

pub(super) fn encode(format: Format, pcm: &[f32], rate: u32) -> Result<Vec<u8>, String> {
    if pcm.iter().any(|sample| !sample.is_finite()) {
        return Err("Export contains non-finite audio samples".into());
    }
    let samples = if rate == crate::app::render::SAMPLE_RATE {
        pcm.to_vec()
    } else {
        crate::app::audio::resample(pcm, crate::app::render::SAMPLE_RATE, rate)?
    };
    let integers: Vec<i32> = samples
        .iter()
        .map(|&sample| {
            let value = if sample <= -1.0 {
                i32::MIN
            } else if sample >= 1.0 {
                i32::MAX
            } else {
                ((f64::from(sample) * 2147483647.0 + 6755399441055744.0).to_bits() as u32) as i32
            };
            value >> 16
        })
        .collect();
    match format {
        Format::Wav => {
            let data_size = integers
                .len()
                .checked_mul(2)
                .and_then(|size| u32::try_from(size).ok())
                .ok_or("WAV data exceeds the RIFF size limit")?;
            let riff_size = data_size
                .checked_add(36)
                .ok_or("WAV exceeds the RIFF size limit")?;
            let mut bytes = Vec::with_capacity(data_size as usize + 44);
            bytes.extend_from_slice(b"RIFF");
            bytes.extend_from_slice(&riff_size.to_le_bytes());
            bytes.extend_from_slice(b"WAVEfmt ");
            bytes.extend_from_slice(&16u32.to_le_bytes());
            bytes.extend_from_slice(&1u16.to_le_bytes());
            bytes.extend_from_slice(&1u16.to_le_bytes());
            bytes.extend_from_slice(&rate.to_le_bytes());
            bytes.extend_from_slice(&(rate * 2).to_le_bytes());
            bytes.extend_from_slice(&2u16.to_le_bytes());
            bytes.extend_from_slice(&16u16.to_le_bytes());
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&data_size.to_le_bytes());
            for sample in integers {
                bytes.extend_from_slice(&(sample as i16).to_le_bytes());
            }
            Ok(bytes)
        }
        Format::Flac => {
            let mut configuration = flacenc::config::Encoder::default();
            configuration.block_size = 1152;
            configuration.subframe_coding.use_lpc = false;
            let block_size = configuration.block_size;
            let configuration = configuration
                .into_verified()
                .map_err(|error| format!("FLAC configuration: {error:?}"))?;
            let source = flacenc::source::MemSource::from_samples(&integers, 1, 16, rate as usize);
            let stream = flacenc::encode_with_fixed_block_size(&configuration, source, block_size)
                .map_err(|error| format!("FLAC encoding: {error:?}"))?;
            let mut sink = flacenc::bitsink::ByteSink::new();
            stream
                .write(&mut sink)
                .map_err(|error| format!("FLAC output: {error:?}"))?;
            Ok(sink.as_slice().to_vec())
        }
    }
}
