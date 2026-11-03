//! Uncompressed RIFF/WAVE export with explicit quantization and no normalization.
use crate::{Result, invalid, tensor::Tensor};
use std::io::Write;
#[derive(Clone, Copy, Debug)]
pub enum Encoding {
    Pcm16,
    Pcm24,
    Float32,
}
pub fn write(
    writer: &mut impl Write,
    samples: &Tensor,
    sample_rate: u32,
    encoding: Encoding,
) -> Result<()> {
    if !(8000..=384000).contains(&sample_rate)
        || samples.cols() > 32
        || samples.rows() > u32::MAX as usize
    {
        return Err(invalid("WAVE dimensions or sample rate"));
    }
    samples.ensure_finite()?;
    let (tag, bits, bytes, float) = match encoding {
        Encoding::Pcm16 => (1u16, 16u16, 2usize, false),
        Encoding::Pcm24 => (1, 24, 3, false),
        Encoding::Float32 => (3, 32, 4, true),
    };
    let data_bytes = samples
        .values()
        .len()
        .checked_mul(bytes)
        .ok_or_else(|| invalid("WAVE data overflow"))?;
    let fmt_len = if float { 18u32 } else { 16 };
    let fact_len = if float { 12u32 } else { 0 };
    let size = 4u64
        + 8
        + fmt_len as u64
        + fact_len as u64
        + 8
        + data_bytes as u64
        + (data_bytes % 2) as u64;
    if size > u32::MAX as u64 {
        return Err(invalid("WAVE exceeds RIFF size limit"));
    }
    writer.write_all(b"RIFF")?;
    writer.write_all(&(size as u32).to_le_bytes())?;
    writer.write_all(b"WAVEfmt ")?;
    writer.write_all(&fmt_len.to_le_bytes())?;
    writer.write_all(&tag.to_le_bytes())?;
    writer.write_all(&(samples.cols() as u16).to_le_bytes())?;
    writer.write_all(&sample_rate.to_le_bytes())?;
    let alignment = (samples.cols() * bytes) as u16;
    writer.write_all(&(sample_rate * alignment as u32).to_le_bytes())?;
    writer.write_all(&alignment.to_le_bytes())?;
    writer.write_all(&bits.to_le_bytes())?;
    if float {
        writer.write_all(&0u16.to_le_bytes())?;
        writer.write_all(b"fact")?;
        writer.write_all(&4u32.to_le_bytes())?;
        writer.write_all(&(samples.rows() as u32).to_le_bytes())?;
    }
    writer.write_all(b"data")?;
    writer.write_all(&(data_bytes as u32).to_le_bytes())?;
    for sample in samples.values() {
        match encoding {
            Encoding::Float32 => writer.write_all(&sample.to_le_bytes())?,
            Encoding::Pcm16 => writer.write_all(
                &((sample.clamp(-1.0, 1.0) as f64 * 32768.0)
                    .round()
                    .clamp(-32768.0, 32767.0) as i16)
                    .to_le_bytes(),
            )?,
            Encoding::Pcm24 => writer.write_all(
                &((sample.clamp(-1.0, 1.0) as f64 * 8388608.0)
                    .round()
                    .clamp(-8388608.0, 8388607.0) as i32)
                    .to_le_bytes()[..3],
            )?,
        }
    }
    if data_bytes % 2 != 0 {
        writer.write_all(&[0])?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn riff_float_words_and_pcm24_padding() {
        let x = Tensor::new(1, 1, vec![-0.0]).unwrap();
        let mut bytes = Vec::new();
        write(&mut bytes, &x, 48000, Encoding::Float32).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize,
            bytes.len() - 8
        );
        assert_eq!(&bytes[bytes.len() - 4..], &(-0.0f32).to_le_bytes());
        assert_eq!(&bytes[38..42], b"fact");
        let mut bytes = Vec::new();
        write(
            &mut bytes,
            &Tensor::new(1, 1, vec![1.0]).unwrap(),
            48000,
            Encoding::Pcm24,
        )
        .unwrap();
        assert_eq!(&bytes[44..], &[255, 255, 127, 0]);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 3);
    }
}
