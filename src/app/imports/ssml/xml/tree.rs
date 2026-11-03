//! Owned XML nodes, including children injected by textual entities.
use super::{
    Entities, attributes, element_name, preflight, space, tag_end, validation_copy, xml_space,
};

pub(crate) struct Element {
    pub name: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Child>,
}

impl Element {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

pub(crate) enum Child {
    Text(String),
    Element(Element),
}

#[derive(Default)]
struct Budget {
    nodes: usize,
    bytes: usize,
    parsed_nodes: usize,
    parsed_bytes: usize,
    expansion_depth: usize,
}

impl Budget {
    fn node(&mut self) -> Result<(), String> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .filter(|n| *n <= 250_000)
            .ok_or("SSML XML node budget")?;
        Ok(())
    }

    fn bytes(&mut self, count: usize) -> Result<(), String> {
        self.bytes = self
            .bytes
            .checked_add(count)
            .filter(|n| *n <= 64 * 1024 * 1024)
            .ok_or("SSML XML string budget")?;
        Ok(())
    }

    fn text(&mut self, text: String, output: &mut Vec<Child>) -> Result<(), String> {
        self.node()?;
        self.bytes(text.len())?;
        output.push(Child::Text(text));
        Ok(())
    }
}

pub(super) fn parse(source: &str) -> Result<Element, String> {
    let doctype = preflight(source)?;
    let body = doctype
        .as_ref()
        .map(|range| &source[range.start + 9..range.end - 1]);
    let entities = Entities::new(body)?;
    from_source(
        source,
        doctype.as_ref(),
        &entities,
        &mut Budget::default(),
        1,
    )
}

fn from_source(
    source: &str,
    doctype: Option<&std::ops::Range<usize>>,
    entities: &Entities,
    budget: &mut Budget,
    depth: usize,
) -> Result<Element, String> {
    if depth > 64 {
        return Err("SSML XML depth exceeds 64".into());
    }
    budget.parsed_bytes = budget
        .parsed_bytes
        .checked_add(source.len())
        .filter(|n| *n <= 64 * 1024 * 1024)
        .ok_or("SSML XML parse-byte budget")?;
    let validation = validation_copy(source, doctype)?;
    let remaining = 250_000usize
        .checked_sub(budget.parsed_nodes)
        .ok_or("SSML XML parsed-node budget")?;
    let document = roxmltree::Document::parse_with_options(
        &validation,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            nodes_limit: remaining as u32,
        },
    )
    .map_err(|error| format!("Invalid SSML XML: {error}"))?;
    budget.parsed_nodes = budget
        .parsed_nodes
        .checked_add(document.descendants().count())
        .filter(|n| *n <= 250_000)
        .ok_or("SSML XML parsed-node budget")?;
    element(document.root_element(), source, entities, budget, depth)
}

fn element(
    node: roxmltree::Node<'_, '_>,
    source: &str,
    entities: &Entities,
    budget: &mut Budget,
    depth: usize,
) -> Result<Element, String> {
    if depth > 64 {
        return Err("SSML XML depth exceeds 64".into());
    }
    budget.node()?;
    let name = element_name(node, source)?;
    budget.bytes(name.len())?;
    let attributes = attributes(node, source, entities)?;
    for (name, value) in &attributes {
        budget.node()?;
        budget.bytes(name.len())?;
        budget.bytes(value.len())?;
    }
    let mut children = Vec::new();
    let range = node.range();
    let start = tag_end(source, range.start)?;
    if !source[range.start..start - 1]
        .trim_end_matches(xml_space)
        .ends_with('/')
    {
        let end = range.start
            + source[range.clone()]
                .rfind("</")
                .ok_or("Missing XML closing tag")?;
        let mut cursor = start;
        for child in node.children().filter(roxmltree::Node::is_element) {
            let range = child.range();
            text_nodes(
                source
                    .get(cursor..range.start)
                    .ok_or("Invalid XML child span")?,
                entities,
                budget,
                depth,
                &mut children,
            )?;
            children.push(Child::Element(element(
                child,
                source,
                entities,
                budget,
                depth + 1,
            )?));
            cursor = range.end;
        }
        text_nodes(
            source.get(cursor..end).ok_or("Invalid XML content span")?,
            entities,
            budget,
            depth,
            &mut children,
        )?;
    }
    Ok(Element {
        name,
        attributes,
        children,
    })
}

fn text_nodes(
    mut gap: &str,
    entities: &Entities,
    budget: &mut Budget,
    depth: usize,
    output: &mut Vec<Child>,
) -> Result<(), String> {
    let mut pending = String::new();
    while !gap.is_empty() {
        if let Some(tail) = gap.strip_prefix("<!--") {
            let end = tail.find("-->").ok_or("Unclosed XML comment")?;
            gap = &tail[end + 3..];
        } else if let Some(tail) = gap.strip_prefix("<?") {
            flush_text(&mut pending, budget, output)?;
            let end = tail.find("?>").ok_or("Unclosed XML instruction")?;
            gap = &tail[end + 2..];
        } else if let Some(tail) = gap.strip_prefix("<![CDATA[") {
            flush_text(&mut pending, budget, output)?;
            let end = tail.find("]]>").ok_or("Unclosed XML CDATA")?;
            budget.text(tail[..end].into(), output)?;
            gap = &tail[end + 3..];
        } else {
            let end = gap.find('<').unwrap_or(gap.len());
            if end == 0 {
                return Err("Unexpected XML content boundary".into());
            }
            ordinary(&gap[..end], entities, budget, depth, &mut pending, output)?;
            gap = &gap[end..];
        }
    }
    flush_text(&mut pending, budget, output)
}

fn ordinary(
    mut text: &str,
    entities: &Entities,
    budget: &mut Budget,
    depth: usize,
    pending: &mut String,
    output: &mut Vec<Child>,
) -> Result<(), String> {
    while let Some(start) = text.find('&') {
        literal(&text[..start], pending);
        text = &text[start + 1..];
        let end = text.find(';').ok_or("Unclosed XML entity")?;
        let value = entities.reference(&text[..end])?;
        if value.starts_with('<') && value.chars().nth(1).is_some() {
            // Injected children precede the still-pending ordinary text node.
            let mut cursor = 0;
            while let Some(range) = fragment(&value, &mut cursor)? {
                let source = &value[range];
                let doctype = preflight(source)?;
                if doctype.is_some() {
                    return Err("DOCTYPE is not an element-valued entity".into());
                }
                if budget.expansion_depth >= 32 {
                    return Err("SSML entity recursion budget".into());
                }
                budget.expansion_depth += 1;
                let child = from_source(source, None, entities, budget, depth + 1);
                budget.expansion_depth -= 1;
                output.push(Child::Element(child?));
            }
        } else {
            pending.push_str(&value);
        }
        if pending.len() > 64 * 1024 * 1024 {
            return Err("SSML XML text budget".into());
        }
        text = &text[end + 1..];
    }
    literal(text, pending);
    if pending.len() > 64 * 1024 * 1024 {
        return Err("SSML XML text budget".into());
    }
    Ok(())
}

fn literal(text: &str, output: &mut String) {
    output.push_str(&text.replace("\r\n", "\n").replace('\r', "\n"));
}

fn flush_text(
    pending: &mut String,
    budget: &mut Budget,
    output: &mut Vec<Child>,
) -> Result<(), String> {
    if pending.chars().any(|ch| !space(ch)) {
        budget.text(std::mem::take(pending), output)?;
    } else {
        pending.clear();
    }
    Ok(())
}

fn fragment(source: &str, cursor: &mut usize) -> Result<Option<std::ops::Range<usize>>, String> {
    loop {
        let tail = &source[*cursor..];
        *cursor += tail.len() - tail.trim_start_matches(space).len();
        let tail = &source[*cursor..];
        if tail.starts_with("<!--") {
            *cursor += tail.find("-->").ok_or("Unclosed entity comment")? + 3;
        } else if tail.starts_with("<?") {
            *cursor += tail.find("?>").ok_or("Unclosed entity instruction")? + 2;
        } else {
            break;
        }
    }
    if !source[*cursor..].starts_with('<') {
        return Ok(None);
    }
    let start = *cursor;
    let (mut scan, mut depth) = (start, 0usize);
    while scan < source.len() {
        let tail = &source[scan..];
        if tail.starts_with("<!--") {
            scan += tail.find("-->").ok_or("Unclosed entity comment")? + 3;
        } else if tail.starts_with("<![CDATA[") && depth != 0 {
            scan += tail.find("]]>").ok_or("Unclosed entity CDATA")? + 3;
        } else if tail.starts_with("<?") {
            scan += tail.find("?>").ok_or("Unclosed entity instruction")? + 2;
        } else if tail.starts_with("<!") {
            return Err("Invalid element-valued entity declaration".into());
        } else if tail.starts_with('<') {
            let end = tag_end(source, scan)?;
            if tail.starts_with("</") {
                depth = depth.checked_sub(1).ok_or("Invalid entity closing tag")?;
            } else if !source[scan..end - 1]
                .trim_end_matches(xml_space)
                .ends_with('/')
            {
                depth = depth
                    .checked_add(1)
                    .filter(|n| *n <= 64)
                    .ok_or("SSML XML depth exceeds 64")?;
            }
            scan = end;
            if depth == 0 {
                *cursor = scan;
                return Ok(Some(start..scan));
            }
        } else {
            scan += tail.find('<').unwrap_or(tail.len());
        }
    }
    Err("Unclosed element-valued entity".into())
}
