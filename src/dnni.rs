//! Bounded plain DNNI v1/v2 reader. Unknown aliases remain inspectable.
mod tags;
use crate::{
    Result, invalid,
    tensor::{Tensor, checked_elements, floats, u32_at},
};

// The supplied unified language model is 95,099,370 bytes.
pub const MAX_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_NODES: usize = 100_000;
pub const MAX_DEPTH: usize = 64;

#[derive(Debug)]
pub struct Node<'a> {
    pub tag: String,
    pub composite: bool,
    pub payload: &'a [u8],
    pub children: Vec<Node<'a>>,
    pub offset: usize,
}

pub fn parse(bytes: &[u8]) -> Result<Node<'_>> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid("DNNI byte budget exceeded"));
    }
    if u32_at(bytes, 0)? != 0x7fca00ff {
        return Err(invalid("not a plain DNNI file"));
    }
    let version = u32_at(bytes, 4)?;
    if !matches!(version, 1 | 2) {
        return Err(invalid("only plain DNNI v1/v2 are supported"));
    }
    let mut offset = 8;
    let mut nodes = 0;
    let root = read_node(bytes, &mut offset, &mut nodes, 0, version)?;
    if offset != bytes.len() {
        return Err(invalid("trailing DNNI bytes"));
    }
    Ok(root)
}

fn read_node<'a>(
    bytes: &'a [u8],
    at: &mut usize,
    count: &mut usize,
    depth: usize,
    version: u32,
) -> Result<Node<'a>> {
    if depth > MAX_DEPTH || *count >= MAX_NODES {
        return Err(invalid("DNNI depth/node budget exceeded"));
    }
    *count += 1;
    let start = *at;
    let marker = u32_at(bytes, start)?;
    if !matches!(marker, 0x7fca40ff | 0x7fca41ff) {
        return Err(invalid(format!("invalid node marker at {start}")));
    }
    let hdr = bytes
        .get(start..start + 20)
        .ok_or_else(|| invalid("truncated node header"))?;
    let raw = &hdr[4..12];
    let end = raw.iter().position(|x| *x == 0).unwrap_or(8);
    if version == 1
        && (end == 0
            || !raw[..end].iter().all(|x| x.is_ascii_graphic())
            || raw[end..].iter().any(|x| *x != 0))
    {
        return Err(invalid("invalid DNNI v1 tag"));
    }
    let identity = u64::from_le_bytes(raw.try_into().unwrap());
    let tag = if version == 1 {
        String::from_utf8(raw[..end].to_vec()).map_err(|_| invalid("tag encoding"))?
    } else {
        tags::resolve(identity)
    };
    let stored = u32_at(hdr, 12)?;
    let child_count = if version == 1 {
        stored
    } else {
        ((identity as u32) ^ ((identity >> 32) as u32) ^ stored ^ 0xac5e7bd5) / 3
    } as usize;
    let length = u32_at(hdr, 16)? as usize;
    let payload_end = (start + 20)
        .checked_add(length)
        .ok_or_else(|| invalid("payload overflow"))?;
    let payload = bytes
        .get(start + 20..payload_end)
        .ok_or_else(|| invalid("truncated node payload"))?;
    if child_count > (bytes.len() - payload_end) / 20 || child_count > MAX_NODES - *count {
        return Err(invalid("impossible child count"));
    }
    // The explicit child count and exact file consumption determine traversal.
    *at = payload_end;
    let mut children = Vec::new();
    for _ in 0..child_count {
        children.push(read_node(bytes, at, count, depth + 1, version)?);
    }
    Ok(Node {
        tag,
        composite: marker == 0x7fca41ff,
        payload,
        children,
        offset: start,
    })
}

impl Node<'_> {
    pub fn tensor(&self) -> Result<Tensor> {
        if !self.children.is_empty() {
            return Err(invalid("tensor node has children"));
        }
        if self.tag == "prim4" {
            return self.dequantized();
        }
        let (rows, cols, header) = match self.tag.as_str() {
            "prim0" => (
                u32_at(self.payload, 0)? as usize,
                u32_at(self.payload, 4)? as usize,
                8,
            ),
            "prim1" => (1, u32_at(self.payload, 0)? as usize, 4),
            _ => return Err(invalid(format!("unsupported tensor {}", self.tag))),
        };
        let n = checked_elements(rows, cols)?;
        if self.payload.len() != header + n * 4 {
            return Err(invalid("tensor payload exact length mismatch"));
        }
        Tensor::new(rows, cols, floats(&self.payload[header..]))
    }
    /// Float dequantization is an explicit arithmetic policy. This does not
    /// emulate the original engine's possible integer accumulation path.
    fn dequantized(&self) -> Result<Tensor> {
        let bits = u32_at(self.payload, 0)? as usize;
        let reserved = u32_at(self.payload, 8)?;
        let scales = u32_at(self.payload, 12)? as usize;
        if !matches!(bits, 8 | 16) || reserved != 0 || scales > crate::tensor::MAX_ELEMENTS {
            return Err(invalid("unsupported quantized tensor configuration"));
        }
        let header = 16 + 4 * scales;
        let rows = u32_at(self.payload, header)? as usize;
        let cols = u32_at(self.payload, header + 4)? as usize;
        let n = checked_elements(rows, cols)?;
        if scales != rows || self.payload.len() != header + 8 + n * (bits / 8) {
            return Err(invalid("quantized tensor scale/length mismatch"));
        }
        let scale = floats(&self.payload[16..header]);
        if scale.iter().any(|v| !v.is_finite()) {
            return Err(invalid("non-finite quantized scale"));
        }
        let packed = &self.payload[header + 8..];
        let divisor = if bits == 8 { 128.0 } else { 32768.0 };
        let mut values = vec![0.0; n];
        for c in 0..cols {
            for r in 0..rows {
                let at = c * rows + r;
                let q = if bits == 8 {
                    packed[at] as i8 as f32
                } else {
                    i16::from_le_bytes([packed[2 * at], packed[2 * at + 1]]) as f32
                };
                values[r * cols + c] = q * (scale[r] / divisor);
            }
        }
        Tensor::new(rows, cols, values)
    }
    pub fn inventory(&self, depth: usize, lines: &mut Vec<String>) {
        lines.push(format!(
            "{}{} offset={} payload={} children={}",
            "  ".repeat(depth),
            self.tag,
            self.offset,
            self.payload.len(),
            self.children.len()
        ));
        for c in &self.children {
            c.inventory(depth + 1, lines);
        }
    }
}
