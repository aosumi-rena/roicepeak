//! Dictionary accent metadata, phrase formation and compound accent arithmetic.
use super::Word;
use crate::{Result, invalid};

pub(super) fn pattern(count: usize, nucleus: i32) -> Vec<bool> {
    (0..count)
        .map(|i| {
            if nucleus == 0 {
                i > 0
            } else {
                (i as i32) < nucleus && (nucleus == 1 || i > 0)
            }
        })
        .collect()
}

#[derive(Default)]
pub(super) struct Connection {
    name: String,
    first: i32,
    second: i32,
}

fn integer(value: &str) -> Result<i32> {
    let number: i32 = value
        .trim()
        .parse()
        .map_err(|_| invalid(format!("invalid Japanese accent integer {value:?}")))?;
    if !(-1_000_000..=1_000_000).contains(&number) {
        return Err(invalid("Japanese accent integer budget"));
    }
    Ok(number)
}

fn connection(input: &str, context: &str) -> Result<Connection> {
    let mut result = Connection::default();
    if input.is_empty() || input == "*" {
        return Ok(result);
    }
    // A context-qualified code may contain a comma between its two integer
    // parameters. A new clause starts only at a new F/P/C code or POS prefix.
    let mut rest = input;
    while !rest.is_empty() {
        rest = rest.trim_start_matches([',', ' ', '\t']);
        if rest.is_empty() {
            break;
        }
        let code_at = rest
            .char_indices()
            .find(|(_, c)| matches!(c, 'F' | 'P' | 'C'))
            .map(|(i, _)| i)
            .ok_or_else(|| invalid("unrecognized Japanese accent connection clause"))?;
        let prefix = &rest[..code_at];
        let applies = if prefix.is_empty() {
            true
        } else if let Some(pos) = prefix.strip_suffix('%') {
            pos == context
        } else {
            return Err(invalid("invalid accent connection context"));
        };
        let mut end = code_at + 1;
        while rest.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == code_at + 1 {
            return Err(invalid("accent connection code has no number"));
        }
        let name = &rest[code_at..end];
        if !matches!(
            name,
            "C1" | "C2"
                | "C3"
                | "C4"
                | "C5"
                | "P1"
                | "P2"
                | "P4"
                | "P6"
                | "P13"
                | "P14"
                | "F1"
                | "F2"
                | "F3"
                | "F4"
                | "F5"
                | "F6"
        ) {
            return Err(invalid(format!(
                "unsupported Japanese accent connection {name}"
            )));
        }
        let mut first = 0;
        let mut second = 0;
        if rest.as_bytes().get(end) == Some(&b'@') {
            let start = end + 1;
            end = start;
            if matches!(rest.as_bytes().get(end), Some(b'-' | b'+')) {
                end += 1;
            }
            while rest.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            first = integer(&rest[start..end])?;
            if name == "F6" && rest.as_bytes().get(end) == Some(&b',') {
                let start = end + 1;
                end = start;
                if matches!(rest.as_bytes().get(end), Some(b'-' | b'+')) {
                    end += 1;
                }
                while rest.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
                    end += 1;
                }
                second = integer(&rest[start..end])?;
            }
        }
        if applies {
            result = Connection {
                name: name.into(),
                first,
                second,
            };
        }
        rest = &rest[end..];
    }
    Ok(result)
}

fn resolve(word: &mut Word, context: &str) -> Result<()> {
    let value = word.accent_type.as_str();
    if !value.is_empty() && value != "*" {
        if value.starts_with(['H', 'L']) {
            let (marks, n) = value
                .split_once('@')
                .map_or((value, None), |(a, b)| (a, Some(b)));
            if !marks.chars().all(|c| matches!(c, 'H' | 'L')) {
                return Err(invalid("invalid explicit Japanese accent pattern"));
            }
            word.explicit = Some(marks.into());
            if let Some(n) = n {
                word.accent = integer(n)?;
            }
        } else {
            word.accent = integer(value.split(',').next().unwrap())?;
        }
    }
    word.connection = connection(&word.connection_type, context)?;
    if !word.modification.is_empty() && word.modification != "*" {
        let (kind, value) = word
            .modification
            .split_once('@')
            .unwrap_or((&word.modification, "0"));
        if !matches!(kind, "M1" | "M2" | "M4") {
            return Err(invalid(format!(
                "unsupported Japanese accent modification {kind}"
            )));
        }
        word.mod_kind = kind.into();
        word.mod_value = integer(value)?;
    }
    Ok(())
}

fn modify(word: &mut Word) {
    if word.mod_kind == "M1" || (word.mod_kind == "M2" && word.accent == 0) {
        word.accent = word.morae.len() as i32 - word.mod_value;
    } else if word.mod_kind == "M4" && word.accent >= 2 {
        word.accent -= word.mod_value;
    }
}

fn function_word(w: &Word) -> bool {
    matches!(w.pos.as_str(), "助詞" | "助動詞")
}
fn noun(w: &Word) -> bool {
    matches!(w.pos.as_str(), "名詞" | "代名詞")
}

fn compound(a: &Word, b: &Word) -> bool {
    if (b.pos == "名詞" && b.detail == "固有名詞") || b.pos == "代名詞" {
        return false;
    }
    if b.detail == "数詞" && !(a.detail == "数詞" || a.pos == "接頭辞") {
        return false;
    }
    (noun(a) && noun(b))
        || (a.pos == "接尾辞" && noun(b))
        || (a.detail == "数詞" && b.pos == "接頭辞")
        || b.pos == "接尾辞"
        || a.pos == "接頭辞"
}

fn merge(words: &[Word], a: &[usize], b: &[usize]) -> bool {
    if a.len() == 1 && words[b[0]].pos == "動詞" && words[b[0]].detail == "非自立可能" {
        return true;
    }
    if b.len() != 2 || words[b[0]].detail != "非自立可能" {
        return false;
    }
    let tail = &words[*a.last().unwrap()];
    (tail.detail == "接続助詞"
        && matches!(words[b[0]].text.as_str(), "おり" | "い")
        && words[b[1]].text == "ます")
        || (tail.detail == "格助詞" && words[b[0]].text == "し" && words[b[1]].text == "て")
}

pub(super) fn assign(words: &mut [Word]) -> Result<()> {
    let mut chunks: Vec<Vec<usize>> = Vec::new();
    let mut boundary = true;
    for i in 0..words.len() {
        if !boundary
            && (function_word(&words[i])
                || compound(&words[*chunks.last().unwrap().last().unwrap()], &words[i]))
        {
            chunks.last_mut().unwrap().push(i);
        } else {
            chunks.push(vec![i]);
        }
        boundary = words[i].boundary();
    }
    let mut at = 0;
    while at < chunks.len() {
        let mut indices = chunks[at].clone();
        if at + 1 < chunks.len()
            && !(indices.len() == 1
                && (words[indices[0]].boundary() || words[indices[0]].pos == "補助記号"))
            && merge(words, &indices, &chunks[at + 1])
        {
            at += 1;
            indices.extend_from_slice(&chunks[at]);
        }
        apply(words, &indices)?;
        at += 1;
    }
    Ok(())
}

fn apply(words: &mut [Word], indices: &[usize]) -> Result<()> {
    let mut context = String::new();
    for i in indices {
        let w = &mut words[*i];
        if w.boundary() {
            continue;
        }
        resolve(w, &context)?;
        if w.pos != "接尾辞" && !function_word(w) {
            context.clone_from(&w.pos);
        } else if w.pos == "助動詞" {
            context = "動詞".into();
        }
        if w.pos == "代名詞" {
            context = "名詞".into();
        }
    }
    modify(&mut words[indices[0]]);
    let mut accent = words[indices[0]].accent;
    let mut size = words[indices[0]].morae.len() as i32;
    for j in 1..indices.len() {
        if j + 1 < indices.len() {
            modify(&mut words[indices[j]]);
        }
        let previous = &words[indices[j - 1]];
        let w = &words[indices[j]];
        let edge = w.accent == 0 || w.accent == w.morae.len() as i32;
        accent = match previous.connection.name.as_str() {
            "P1" => {
                if edge {
                    0
                } else {
                    w.accent + size
                }
            }
            "P2" => {
                if edge {
                    size + 1
                } else {
                    w.accent + size
                }
            }
            "P4" => {
                if edge {
                    size + 1
                } else {
                    previous.accent
                }
            }
            "P6" => 0,
            "P13" => previous.accent,
            "P14" => {
                if edge {
                    size
                } else {
                    w.accent + size
                }
            }
            _ => match w.connection.name.as_str() {
                "C1" => w.accent + size,
                "C2" => size + 1,
                "C3" => size,
                "C4" | "F5" => 0,
                "F2" if accent == 0 => w.connection.first + size,
                "F3" if accent != 0 => w.connection.first + size,
                "F4" => w.connection.first + size,
                "F6" => {
                    size + if accent == 0 {
                        w.connection.first
                    } else {
                        w.connection.second
                    }
                }
                _ => accent,
            },
        };
        size += w.morae.len() as i32;
    }
    let marks = pattern(size as usize, accent);
    let mut start = 0;
    for i in indices {
        let w = &mut words[*i];
        let end = start + w.morae.len();
        w.marks = marks[start..end].to_vec();
        if let Some(explicit) = &w.explicit {
            for (slot, c) in w.marks.iter_mut().zip(explicit.chars()) {
                *slot = c == 'H';
            }
            w.fixed = true;
        }
        start = end;
    }
    Ok(())
}
