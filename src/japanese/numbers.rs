//! Sentence-number spelling and a bounded set of spoken counter alternations.
use super::Word;
use crate::{Result, invalid};

fn digit(c: char) -> Option<u32> {
    if c.is_ascii_digit() {
        Some(c as u32 - '0' as u32)
    } else if ('０'..='９').contains(&c) {
        Some(c as u32 - '０' as u32)
    } else {
        None
    }
}

fn digit_spelling(c: char) -> String {
    match digit(c) {
        Some(0) => "ゼロ".into(),
        Some(n) => char::from_u32('０' as u32 + n).unwrap().to_string(),
        None => c.to_string(),
    }
}

fn span(input: &[char]) -> Result<String> {
    if input.is_empty() {
        return Ok(String::new());
    }
    let spelled = || input.iter().map(|c| digit_spelling(*c)).collect::<String>();
    if input.len() <= 1 && input.first().is_some_and(|c| digit(*c).is_none()) {
        return Ok(input.iter().collect());
    }
    if input.iter().skip(1).any(|c| matches!(c, '-' | '‐' | '―'))
        || input.iter().filter(|c| matches!(c, '.' | '．')).count() > 1
        || (input.len() > 1 && digit(input[0]) == Some(0) && !matches!(input[1], '.' | '．'))
    {
        return Ok(spelled());
    }
    let mut at = 0;
    let mut result = String::new();
    if matches!(input.first(), Some('-' | '‐' | '―')) {
        result.push_str("マイナス");
        at += 1;
    }
    let end = input[at..]
        .iter()
        .position(|c| matches!(c, '.' | '．'))
        .map_or(input.len(), |i| at + i);
    if end == at {
        result.extend(input[at..].iter());
        return Ok(result);
    }
    let digits = input[at..end]
        .iter()
        .map(|c| digit(*c).ok_or_else(|| invalid("invalid numeric span")))
        .collect::<Result<Vec<_>>>()?;
    if digits.len() > 72 {
        return Err(invalid(
            "number exceeds supported Japanese large units (72 digits)",
        ));
    }
    let names = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
    let units = [
        "",
        "万",
        "億",
        "兆",
        "京",
        "垓",
        "秭",
        "穰",
        "溝",
        "澗",
        "正",
        "載",
        "極",
        "恒河沙",
        "阿僧祇",
        "那由多",
        "不可思議",
        "無量大数",
    ];
    if digits.len() == 1 {
        result.push(if digits[0] == 0 {
            '零'
        } else {
            names[digits[0] as usize]
        });
    } else {
        let groups = digits.len().div_ceil(4);
        let first = (digits.len() - 1) % 4 + 1;
        let mut left = 0;
        for g in 0..groups {
            let width = if g == 0 { first } else { 4 };
            let current = &digits[left..left + width];
            left += width;
            let unit = groups - g - 1;
            if current.iter().all(|d| *d == 0) {
                continue;
            }
            for (i, d) in current.iter().enumerate() {
                if *d == 0 {
                    continue;
                }
                let place = current.len() - i - 1;
                if *d != 1 || place == 0 || (place == 3 && unit > 0) {
                    result.push(names[*d as usize]);
                }
                result.push_str(["", "十", "百", "千"][place]);
            }
            result.push_str(units[unit]);
        }
    }
    if end < input.len() {
        result.push('点');
        for c in &input[end + 1..] {
            result.push_str(&digit_spelling(*c));
        }
    }
    Ok(result)
}

/// Expands numeric spans before dictionary lookup. Zero groups are omitted;
/// this is a deliberate conventional spelling, not a claim of native parity.
pub fn expand(input: &str) -> Result<String> {
    if input.len() > 1_048_576 {
        return Err(invalid("Japanese number input byte budget"));
    }
    let mut number = Vec::new();
    let chars: Vec<_> = input.chars().collect();
    let mut result = String::new();
    for (i, c) in chars.iter().enumerate() {
        if digit(*c).is_some() || matches!(c, '-' | '.' | '‐' | '．' | '―') {
            number.push(*c);
        } else if !number.is_empty()
            && matches!(c, ',' | '，')
            && chars.get(i + 1).is_some_and(|n| digit(*n).is_some())
        {
            continue;
        } else {
            result.push_str(&span(&number)?);
            number.clear();
            result.push(*c);
        }
    }
    result.push_str(&span(&number)?);
    if result.len() > 4_194_304 {
        return Err(invalid("expanded sentence byte budget"));
    }
    Ok(result)
}

pub(super) fn expand_mapped(
    input: Vec<super::origin::Marked>,
) -> Result<Vec<super::origin::Marked>> {
    use super::origin::Marked;
    let mut result = Vec::new();
    let mut number: Vec<Marked> = Vec::new();
    let flush = |number: &mut Vec<Marked>, result: &mut Vec<Marked>| -> Result<()> {
        if let Some(first) = number.first() {
            let mut origin = first.origin.clone();
            for item in &number[1..] {
                origin.start = origin.start.min(item.origin.start);
                origin.end = origin.end.max(item.origin.end);
            }
            let chars: Vec<_> = number.iter().map(|item| item.value).collect();
            result.extend(span(&chars)?.chars().map(|value| Marked {
                value,
                origin: origin.clone(),
            }));
            if result.len() > super::origin::MAX_SCALARS {
                return Err(invalid("Normalization provenance expansion budget"));
            }
        }
        number.clear();
        Ok(())
    };
    for (index, item) in input.iter().enumerate() {
        if digit(item.value).is_some() || matches!(item.value, '-' | '.' | '‐' | '．' | '―') {
            number.push(item.clone());
        } else if !number.is_empty()
            && matches!(item.value, ',' | '，')
            && input
                .get(index + 1)
                .is_some_and(|next| digit(next.value).is_some())
        {
            if let Some(last) = number.last_mut() {
                last.origin.start = last.origin.start.min(item.origin.start);
                last.origin.end = last.origin.end.max(item.origin.end);
            }
        } else {
            flush(&mut number, &mut result)?;
            result.push(item.clone());
        }
    }
    flush(&mut number, &mut result)?;
    Ok(result)
}

fn numeral(surface: &str) -> Option<usize> {
    [
        "零", "一", "二", "三", "四", "五", "六", "七", "八", "九", "十",
    ]
    .iter()
    .position(|s| *s == surface)
}

fn replace_initial(reading: &str, source: &str, target: &str) -> String {
    let mut chars = reading.chars();
    let Some(c) = chars.next() else {
        return String::new();
    };
    let head = source
        .chars()
        .position(|k| k == c)
        .and_then(|i| target.chars().nth(i))
        .unwrap_or(c);
    format!("{head}{}", chars.as_str())
}

pub(super) fn adjust_digits(words: &mut [Word]) -> Result<()> {
    let long = [
        "ゼロ",
        "イチ",
        "ニイ",
        "サン",
        "ヨン",
        "ゴオ",
        "ロク",
        "ナナ",
        "ハチ",
        "キュウ",
    ];
    for w in words.iter_mut() {
        if w.text.chars().count() == 1 {
            if let Some(n) = w.text.chars().next().and_then(digit) {
                if w.pronunciation != long[n as usize] {
                    w.set_reading(long[n as usize])?;
                    w.set_accent(if [0, 1, 2, 8].contains(&n) { 2 } else { 1 });
                }
            }
        }
    }
    Ok(())
}

pub(super) fn adjust_counters(words: &mut [Word]) -> Result<()> {
    for i in 0..words.len().saturating_sub(1) {
        let (before, after) = words.split_at_mut(i + 1);
        let w = &mut before[i];
        let counter = &mut after[0];
        if w.detail != "数詞" {
            continue;
        }
        let Some(n) = numeral(&w.text) else {
            continue;
        };
        if matches!(counter.text.as_str(), "本" | "分" | "個" | "こ") {
            let readings = [
                "レエ",
                "イッ",
                "ニ",
                "サン",
                "ヨン",
                "ゴ",
                "ロッ",
                "ナナ",
                "ハッ",
                "キュウ",
                "ジッ",
            ];
            w.set_reading(readings[n])?;
            w.set_accent(i32::from(n != 7));
            let suffix = match counter.text.as_str() {
                "本" => {
                    if [1, 6, 8, 10].contains(&n) {
                        "ポン"
                    } else if n == 3 {
                        "ボン"
                    } else {
                        "ホン"
                    }
                }
                "分" => {
                    if [1, 3, 6, 8, 10].contains(&n) {
                        "プン"
                    } else {
                        "フン"
                    }
                }
                _ => "コ",
            };
            if counter.pronunciation != suffix {
                counter.set_reading(suffix)?;
            }
            w.fixed = true;
            counter.fixed = true;
        } else if counter.text == "時" && n > 0 {
            w.set_reading(
                [
                    "",
                    "イチ",
                    "ニ",
                    "サン",
                    "ヨ",
                    "ゴ",
                    "ロク",
                    "シチ",
                    "ハチ",
                    "ク",
                    "ジュウ",
                ][n],
            )?;
            w.set_accent(if [1, 6, 7, 8].contains(&n) { 2 } else { 1 });
            counter.marks.fill(false);
            w.fixed = true;
            counter.fixed = true;
        } else if counter.text == "回" && n < 10 {
            w.set_reading(
                [
                    "レエ",
                    "イッ",
                    "ニ",
                    "サン",
                    "ヨン",
                    "ゴ",
                    "ロッ",
                    "ナナ",
                    "ハッ",
                    "キュウ",
                ][n],
            )?;
            w.set_accent(if n == 0 || n == 9 { 1 } else { 2 });
            if (1..=8).contains(&n) {
                counter.set_reading("カイ")?;
                counter.set_accent(1);
            }
            w.fixed = true;
            counter.fixed = true;
        } else if counter.pos == "接尾辞" && counter.detail == "名詞的" && n < 10 {
            w.set_reading(
                [
                    "レエ",
                    "イッ",
                    "ニ",
                    "サン",
                    "ヨン",
                    "ゴ",
                    "ロッ",
                    "ナナ",
                    "ハッ",
                    "キュウ",
                ][n],
            )?;
            w.set_accent(i32::from(n != 7));
            let pron = if [1, 6, 8].contains(&n) {
                replace_initial(
                    &counter.pronunciation,
                    "ハヒフヘホバビブベボ",
                    "パピプペポパピプペポ",
                )
            } else if n == 3 {
                replace_initial(
                    &counter.pronunciation,
                    "ハヒフヘホパピプペポ",
                    "バビブベボバビブベボ",
                )
            } else {
                replace_initial(&counter.pronunciation, "パピプペポ", "ハヒフヘホ")
            };
            if pron != counter.pronunciation {
                counter.set_reading(&pron)?;
            }
            counter.marks.fill(false);
            w.fixed = true;
            counter.fixed = true;
        } else if matches!(counter.text.as_str(), "円" | "番") && n == 4 {
            w.set_reading("ヨ")?;
            w.set_accent(1);
        } else if matches!(counter.text.as_str(), "ヶ月" | "か月") {
            let pron = match w.pronunciation.as_str() {
                "イチ" => "イッ",
                "シ" => "ヨン",
                "ロク" => "ロッ",
                "シチ" => "ナナ",
                "ク" => "キュー",
                "ジュー" | "ジュウ" => "ジッ",
                _ => continue,
            };
            w.set_reading(pron)?;
            w.set_accent(0);
        }
    }
    Ok(())
}
