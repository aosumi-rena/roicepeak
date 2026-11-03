//! Literal XML names and bounded structure over original source bytes.
mod tree;
use super::entities::Entities;
use std::ops::Range;
pub(super) use tree::{Child, Element};

pub(super) fn parse(source: &str) -> Result<Element, String> {
    tree::parse(source)
}

fn element_name(node: roxmltree::Node<'_, '_>, source: &str) -> Result<String, String> {
    let tail = source
        .get(node.range().start + 1..)
        .ok_or("Invalid XML element span")?;
    let end = tail
        .find(|ch: char| xml_space(ch) || matches!(ch, '/' | '>'))
        .ok_or("Invalid XML element name")?;
    Ok(tail[..end].to_owned())
}

fn attributes(
    node: roxmltree::Node<'_, '_>,
    source: &str,
    entities: &Entities,
) -> Result<Vec<(String, String)>, String> {
    let mut result = Vec::new();
    for attribute in node.attributes() {
        let raw = source
            .get(attribute.range())
            .ok_or("Invalid XML attribute span")?;
        let (name, value) = raw
            .split_once('=')
            .ok_or("Invalid XML attribute assignment")?;
        let name = name.trim_end_matches(xml_space);
        let value = value.trim_start_matches(xml_space);
        let quote = *value
            .as_bytes()
            .first()
            .ok_or("Invalid XML attribute value")?;
        if !matches!(quote, b'\'' | b'"') || value.as_bytes().last() != Some(&quote) {
            return Err("Invalid XML attribute quotes".into());
        }
        result.push((
            name.to_owned(),
            entities.decode(&value[1..value.len() - 1])?,
        ));
    }
    Ok(result)
}

pub(super) fn trim(text: &str) -> &str {
    text.trim_matches(space)
}
fn space(ch: char) -> bool {
    ch.is_whitespace()
}
fn xml_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\r' | '\n')
}

fn preflight(source: &str) -> Result<Option<Range<usize>>, String> {
    let (mut cursor, mut depth) = (0usize, 0usize);
    let mut doctype = None;
    while let Some(relative) = source[cursor..].find('<') {
        cursor += relative;
        let tail = &source[cursor..];
        if tail.starts_with("<!--") {
            cursor += tail.find("-->").ok_or("Unclosed XML comment")? + 3;
        } else if tail.starts_with("<![CDATA[") {
            cursor += tail.find("]]>").ok_or("Unclosed XML CDATA")? + 3;
        } else if tail.starts_with("<?") {
            cursor += tail.find("?>").ok_or("Unclosed XML instruction")? + 2;
        } else if tail.starts_with("<!DOCTYPE") {
            if depth != 0 || doctype.is_some() {
                return Err("Misplaced or duplicate DOCTYPE".into());
            }
            let end = doctype_end(source, cursor)?;
            doctype = Some(cursor..end);
            cursor = end;
        } else if tail.starts_with("<!") {
            return Err("Unsupported XML declaration".into());
        } else {
            let end = tag_end(source, cursor)?;
            if tail.starts_with("</") {
                depth = depth.saturating_sub(1);
            } else {
                depth = depth
                    .checked_add(1)
                    .filter(|n| *n <= 64)
                    .ok_or("SSML XML depth exceeds 64")?;
                if source[cursor..end - 1]
                    .trim_end_matches(xml_space)
                    .ends_with('/')
                {
                    depth -= 1;
                }
            }
            cursor = end;
        }
        if cursor >= source.len() {
            break;
        }
    }
    Ok(doctype)
}

/// Keep source positions while the DOM checks only the element structure.
fn validation_copy(source: &str, doctype: Option<&Range<usize>>) -> Result<String, String> {
    let mut output = source.as_bytes().to_vec();
    let mut cursor = 0;
    while cursor < source.len() {
        if doctype.is_some_and(|range| range.start == cursor) {
            let range = doctype.unwrap();
            output[range.clone()].fill(b' ');
            cursor = range.end;
            continue;
        }
        let tail = &source[cursor..];
        let skip = if tail.starts_with("<!--") {
            Some(("-->", 3))
        } else if tail.starts_with("<![CDATA[") {
            Some(("]]>", 3))
        } else if tail.starts_with("<?") {
            Some(("?>", 2))
        } else {
            None
        };
        if let Some((ending, length)) = skip {
            cursor += tail.find(ending).ok_or("Unclosed XML lexical section")? + length;
        } else if tail.starts_with('&') {
            let end = cursor + tail.find(';').ok_or("Unclosed XML entity")? + 1;
            output[cursor..end].fill(b'x');
            cursor = end;
        } else {
            cursor += tail.chars().next().ok_or("Invalid XML cursor")?.len_utf8();
        }
    }
    String::from_utf8(output).map_err(|error| error.to_string())
}

fn tag_end(source: &str, start: usize) -> Result<usize, String> {
    let mut quote = None;
    for (relative, byte) in source.as_bytes()[start..].iter().copied().enumerate() {
        match (quote, byte) {
            (Some(open), close) if open == close => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Ok(start + relative + 1),
            _ => {}
        }
    }
    Err("Unclosed XML tag".into())
}

fn doctype_end(source: &str, start: usize) -> Result<usize, String> {
    let mut depth = 1usize;
    for (relative, byte) in source.as_bytes()[start + 9..].iter().copied().enumerate() {
        if byte == b'<' {
            depth = depth
                .checked_add(1)
                .filter(|n| *n <= 64)
                .ok_or("SSML DTD nesting budget")?;
        } else if byte == b'>' {
            depth -= 1;
            if depth == 0 {
                return Ok(start + 9 + relative + 1);
            }
        }
    }
    Err("Unclosed DOCTYPE".into())
}
