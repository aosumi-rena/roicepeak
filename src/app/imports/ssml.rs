//! Ordered speech blocks and pronunciation chunks from an SSML document.
mod entities;
mod number;
mod xml;

use super::{MAX_BLOCKS, MAX_TEXT, check};
use roicepeak::japanese::{SsmlReading, ssml_reading};
use std::ops::Range;

pub struct Script {
    pub blocks: Vec<SpeechBlock>,
}

pub struct SpeechBlock {
    pub voice_index: usize,
    pub text: String,
    pub delay_seconds: f64,
    pub chunks: Vec<Chunk>,
}

pub struct Chunk {
    pub surface: String,
    pub ph: String,
    pub reading: SsmlReading,
    /// Bytes in the assembled display text, before sentence annotation matching.
    pub display_bytes: Range<usize>,
}

#[derive(Default)]
struct Budget {
    strings: usize,
    chunks: usize,
    morae: usize,
    groups: usize,
    phones: usize,
}

impl Budget {
    fn strings(&mut self, size: usize) -> Result<(), String> {
        add(
            &mut self.strings,
            size,
            64 * 1024 * 1024,
            "SSML string budget",
        )
    }

    fn chunk(&mut self, chunk: &Chunk) -> Result<(), String> {
        add(&mut self.chunks, 1, 100_000, "SSML chunk budget")?;
        self.strings(chunk.surface.len())?;
        self.strings(chunk.ph.len())?;
        add(
            &mut self.morae,
            chunk.reading.morae.len(),
            100_000,
            "SSML mora budget",
        )?;
        add(
            &mut self.groups,
            chunk.reading.groups.len(),
            100_000,
            "SSML group budget",
        )?;
        for mora in &chunk.reading.morae {
            self.strings(mora.syllable.reading.len())?;
            add(
                &mut self.phones,
                mora.syllable.phones.len(),
                300_000,
                "SSML phone budget",
            )?;
            for phone in &mora.syllable.phones {
                self.strings(phone.symbol.len())?;
            }
        }
        Ok(())
    }
}

fn add(total: &mut usize, count: usize, limit: usize, message: &str) -> Result<(), String> {
    *total = total
        .checked_add(count)
        .filter(|n| *n <= limit)
        .ok_or(message)?;
    Ok(())
}

pub(super) fn parse(source: &str) -> Result<Script, String> {
    check(source)?;
    let root = xml::parse(source)?;
    if root.name != "speak" {
        return Err("SSML root must be the exact tag speak".into());
    }
    let mut script = Script { blocks: Vec::new() };
    let mut budget = Budget::default();
    for (voice_index, voice) in root
        .children
        .iter()
        .filter_map(|child| match child {
            xml::Child::Element(element) if element.name == "voice" => Some(element),
            _ => None,
        })
        .enumerate()
    {
        let mut block = SpeechBlock {
            voice_index,
            text: String::new(),
            delay_seconds: 0.0,
            chunks: Vec::new(),
        };
        for child in &voice.children {
            match child {
                xml::Child::Text(text) => ordinary(&mut block, &mut budget, &text)?,
                xml::Child::Element(node) if node.name == "phoneme" => {
                    if let Some(xml::Child::Text(text)) = node.children.first() {
                        let surface = xml::trim(&text);
                        let display_bytes = append_text(&mut block, &mut budget, surface)?;
                        let alphabet = node.attribute("alphabet").unwrap_or("sapi");
                        let (ph, reading) = if alphabet == "sapi" {
                            let ph = node.attribute("ph").unwrap_or("").to_owned();
                            let reading = ssml_reading(&ph).map_err(|error| error.to_string())?;
                            (ph, reading)
                        } else {
                            (String::new(), SsmlReading::default())
                        };
                        let chunk = Chunk {
                            surface: surface.into(),
                            ph,
                            reading,
                            display_bytes,
                        };
                        budget.chunk(&chunk)?;
                        block.chunks.push(chunk);
                    }
                }
                xml::Child::Element(node) if node.name == "break" => {
                    flush(&mut script, &mut block)?;
                    block.delay_seconds = number::delay(node.attribute("time"));
                }
                xml::Child::Element(_) => {}
            }
        }
        if !block.text.is_empty() {
            flush(&mut script, &mut block)?;
        }
    }
    Ok(script)
}

fn append_text(
    block: &mut SpeechBlock,
    budget: &mut Budget,
    text: &str,
) -> Result<Range<usize>, String> {
    block
        .text
        .len()
        .checked_add(text.len())
        .and_then(|n| n.checked_add(1))
        .filter(|n| *n <= MAX_TEXT)
        .ok_or("SSML block text exceeds 1 MiB")?;
    budget.strings(text.len() + 1)?;
    let start = block.text.len();
    block.text.push_str(text);
    let end = block.text.len();
    block.text.push(' ');
    Ok(start..end)
}

fn ordinary(block: &mut SpeechBlock, budget: &mut Budget, text: &str) -> Result<(), String> {
    let text = xml::trim(text);
    let display = append_text(block, budget, text)?;
    for range in crate::app::sentences::partition(text) {
        let chunk = Chunk {
            surface: text[range.bytes.clone()].into(),
            ph: String::new(),
            reading: SsmlReading::default(),
            display_bytes: display.start + range.bytes.start..display.start + range.bytes.end,
        };
        budget.chunk(&chunk)?;
        block.chunks.push(chunk);
    }
    Ok(())
}

fn flush(script: &mut Script, block: &mut SpeechBlock) -> Result<(), String> {
    if script.blocks.len() >= MAX_BLOCKS {
        return Err("SSML exceeds 10000 speech blocks".into());
    }
    let next = SpeechBlock {
        voice_index: block.voice_index,
        text: String::new(),
        delay_seconds: 0.0,
        chunks: Vec::new(),
    };
    script.blocks.push(std::mem::replace(block, next));
    Ok(())
}
