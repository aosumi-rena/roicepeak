//! Source-level DTD tokens and bounded textual entity expansion.
use std::cell::Cell;

const MAX_TOKENS: usize = 250_000;
const MAX_EXPANSION: usize = 64 * 1024 * 1024;

pub(super) struct Entities {
    tokens: Vec<String>,
    work: Cell<usize>,
}

impl Entities {
    pub fn new(body: Option<&str>) -> Result<Self, String> {
        let mut tokens = Vec::new();
        if let Some(body) = body {
            let body = body.trim_end_matches('>');
            let header = tokenize(body)?;
            let external = header.len() >= 2
                && header[header.len() - 2].eq_ignore_ascii_case("system")
                && header
                    .last()
                    .is_some_and(|value| value.starts_with(['\'', '"']));
            if !external {
                if let Some((open, close)) =
                    body.find('[').zip(body.rfind(']')).filter(|(a, b)| a < b)
                {
                    tokens = tokenize(&body[open + 1..close])?;
                }
            }
        }
        let mut result = Self {
            tokens,
            work: Cell::new(0),
        };
        // Inserted tokens are not revisited by this backward pass.
        for index in (0..result.tokens.len()).rev() {
            let token = &result.tokens[index];
            if let Some(name) = token
                .strip_prefix('%')
                .and_then(|value| value.strip_suffix(';'))
            {
                let replacement = result.parameter(name);
                let replacement = tokenize(&replacement)?;
                let count = result
                    .tokens
                    .len()
                    .checked_sub(1)
                    .and_then(|n| n.checked_add(replacement.len()))
                    .filter(|n| *n <= MAX_TOKENS)
                    .ok_or("SSML DTD token budget")?;
                result.charge(replacement.iter().map(String::len).sum())?;
                result.tokens.splice(index..index + 1, replacement);
                if result.tokens.len() != count {
                    return Err("Invalid DTD token insertion".into());
                }
            }
        }
        Ok(result)
    }

    fn parameter(&self, name: &str) -> String {
        for (index, token) in self.tokens.iter().enumerate() {
            if token == name
                && index >= 2
                && self.tokens[index - 1] == "%"
                && self.tokens[index - 2].eq_ignore_ascii_case("<!entity")
            {
                let value = self
                    .tokens
                    .get(index + 1)
                    .map(String::as_str)
                    .unwrap_or("")
                    .trim_end_matches('>');
                if value.eq_ignore_ascii_case("system") {
                    return String::new();
                }
                return unquote(value.trim()).to_owned();
            }
        }
        name.to_owned()
    }

    pub fn decode(&self, raw: &str) -> Result<String, String> {
        self.expand(raw, &mut Vec::new())
    }

    pub fn reference(&self, name: &str) -> Result<String, String> {
        self.resolve(name, &mut Vec::new())
    }

    fn expand(&self, mut text: &str, stack: &mut Vec<String>) -> Result<String, String> {
        let mut output = String::new();
        while let Some(start) = text.find('&') {
            self.append(&mut output, &text[..start])?;
            text = &text[start + 1..];
            let end = text.find(';').ok_or("Unclosed XML entity")?;
            let value = self.resolve(&text[..end], stack)?;
            self.append(&mut output, &value)?;
            text = &text[end + 1..];
        }
        self.append(&mut output, text)?;
        Ok(output)
    }

    fn resolve(&self, name: &str, stack: &mut Vec<String>) -> Result<String, String> {
        let builtin = if name.eq_ignore_ascii_case("amp") {
            Some('&')
        } else if name.eq_ignore_ascii_case("lt") {
            Some('<')
        } else if name.eq_ignore_ascii_case("gt") {
            Some('>')
        } else if name.eq_ignore_ascii_case("quot") {
            Some('"')
        } else if name.eq_ignore_ascii_case("apos") {
            Some('\'')
        } else {
            None
        };
        if let Some(value) = builtin {
            return Ok(value.to_string());
        }
        if let Some(number) = name.strip_prefix('#') {
            let (number, radix) = number
                .strip_prefix('x')
                .or_else(|| number.strip_prefix('X'))
                .map_or((number, 10), |value| (value, 16));
            let code =
                u32::from_str_radix(number, radix).map_err(|_| "Invalid XML character entity")?;
            let value = char::from_u32(code)
                .filter(|ch| *ch != '\0')
                .ok_or("Invalid XML character entity")?;
            return Ok(value.to_string());
        }
        if name.len() > 4096 || stack.len() >= 32 || stack.iter().any(|item| item == name) {
            return Err("SSML entity recursion or name budget".into());
        }
        for (index, token) in self.tokens.iter().enumerate() {
            if token == name && index > 0 && self.tokens[index - 1].eq_ignore_ascii_case("<!entity")
            {
                let value = self
                    .tokens
                    .get(index + 1)
                    .map(String::as_str)
                    .unwrap_or("")
                    .trim_end_matches('>');
                let value = unquote(value.trim());
                stack.push(name.to_owned());
                let result = self.expand(value, stack);
                stack.pop();
                return result;
            }
        }
        Ok(name.to_owned())
    }

    fn append(&self, output: &mut String, text: &str) -> Result<(), String> {
        self.charge(text.len())?;
        output.push_str(text);
        Ok(())
    }

    fn charge(&self, bytes: usize) -> Result<(), String> {
        let work = self
            .work
            .get()
            .checked_add(bytes)
            .filter(|n| *n <= MAX_EXPANSION)
            .ok_or("SSML entity expansion budget")?;
        self.work.set(work);
        Ok(())
    }
}

fn unquote(value: &str) -> &str {
    let Some(first) = value.as_bytes().first() else {
        return value;
    };
    if !matches!(first, b'\'' | b'"') {
        return value;
    }
    let value = &value[1..];
    if value.ends_with(['\'', '"']) {
        &value[..value.len() - 1]
    } else {
        value
    }
}

fn tokenize(text: &str) -> Result<Vec<String>, String> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let (mut start, mut quoted) = (0usize, false);
    let mut tokens = Vec::new();
    for (byte, character) in text.char_indices() {
        if !quoted && matches!(character, ' ' | '\n' | '\r' | '\t') {
            tokens.push(text[start..byte].to_owned());
            start = byte + character.len_utf8();
            if tokens.len() >= MAX_TOKENS {
                return Err("SSML DTD token budget".into());
            }
        } else if character == '"' {
            quoted = !quoted;
        }
    }
    tokens.push(text[start..].to_owned());
    Ok(tokens)
}
