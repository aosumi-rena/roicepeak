use super::{Syllable, Words, numbers, phone, punctuation, silence};
use pinyin::ToPinyin;
use std::{collections::HashMap, sync::OnceLock};

fn lexicon() -> &'static HashMap<&'static str, &'static str> {
    static LEXICON: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    LEXICON.get_or_init(|| {
        include_str!("../../assets/pinyin-phrases.tsv")
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .collect()
    })
}

pub(super) fn words(text: &str) -> Words {
    let mut words = Vec::new();
    let mut cardinal_ones = Vec::new();
    let characters = text.char_indices().collect::<Vec<_>>();
    let mut index = 0;
    while let Some(&(start, ch)) = characters.get(index) {
        if let Some(number) = numbers::scan(text, start) {
            let end = start + number.end;
            let year = number.integer.len() == 4
                && number.fraction.is_empty()
                && !number.percent
                && text[start..end].chars().count() == 4
                && !text[..start].trim_end().ends_with('第')
                && text[end..].starts_with('年');
            let reading = numeral_text(&number, year);
            let syllables = self::words(&reading)
                .into_iter()
                .flat_map(|(_, syllables)| syllables)
                .collect::<Vec<_>>();
            if !syllables.is_empty() {
                if number.sign.is_none()
                    && number.fraction.is_empty()
                    && !number.percent
                    && !number.integer.starts_with('0')
                    && number.integer.ends_with('1')
                    && number.integer.len() <= 12
                    && !year
                {
                    cardinal_ones.push(words.len());
                }
                words.push((start..end, syllables));
            }
            while characters.get(index).is_some_and(|(byte, _)| *byte < end) {
                index += 1;
            }
            continue;
        }
        let end = start + ch.len_utf8();
        let matched = [3, 2].into_iter().find_map(|len| {
            let &(last, last_char) = characters.get(index + len - 1)?;
            let end = last + last_char.len_utf8();
            let parts = lexicon().get(&text[start..end])?;
            let syllables = parts
                .split_ascii_whitespace()
                .map(syllable)
                .collect::<Option<Vec<_>>>()?;
            Some((len, end, syllables))
        });
        if let Some((len, end, syllables)) = matched {
            words.push((start..end, syllables));
            index += len;
            continue;
        }
        let syllable = if punctuation(ch) {
            Some(silence("pau"))
        } else {
            ch.to_pinyin()
                .and_then(|reading| syllable(reading.with_tone_num_end()))
        };
        if let Some(syllable) = syllable {
            words.push((start..end, vec![syllable]));
        }
        index += 1;
    }
    apply_numeral_boundaries(text, &mut words);
    apply_tones(text, &mut words);
    apply_cardinal_tones(text, &mut words, &cardinal_ones);
    words
}

fn apply_numeral_boundaries(text: &str, words: &mut Words) {
    let is_numeral = |start: usize| text[start..].starts_with(numbers::begins);
    for index in 1..words.len() {
        let (previous, following) = words.split_at_mut(index);
        let (left, syllables) = &mut previous[index - 1];
        let (right, next) = &following[0];
        if is_numeral(left.start) != is_numeral(right.start)
            && text[left.end..right.start].chars().all(char::is_whitespace)
            && next[0].code == 0x3003
            && let Some(last) = syllables
                .last_mut()
                .filter(|syllable| syllable.code == 0x3003)
        {
            last.code = 0x3002;
        }
    }
}

fn apply_tones(text: &str, words: &mut Words) {
    let is_digit = |ch| "零〇一二三四五六七八九".contains(ch);
    let mut readings = words
        .iter_mut()
        .flat_map(|(range, syllables)| {
            let start = range.start;
            let surface = &text[range.clone()];
            let numeral = surface.chars().next().is_some_and(numbers::begins);
            let fixed_reading = syllables.len() > 1 && lexicon().contains_key(surface);
            surface
                .char_indices()
                .zip(syllables)
                .filter(move |_| !numeral)
                .map(move |((offset, ch), syllable)| (start + offset, ch, syllable, fixed_reading))
        })
        .peekable();
    while let Some((position, ch, syllable, fixed_reading)) = readings.next() {
        let end = position + ch.len_utf8();
        let (next_char, next_tone) = readings
            .peek()
            .filter(|(next, _, _, _)| text[end..*next].chars().all(char::is_whitespace))
            .map_or(('\0', 0), |(_, ch, next, _)| (*ch, next.code));
        syllable.code = match (ch, syllable.code, next_tone) {
            ('不', 0x3004, 0x3004) => 0x3002,
            ('一', 0x3001, 0x3001..=0x3004)
                if !fixed_reading
                    && !text[..position].ends_with(|ch| ch == '第' || is_digit(ch))
                    && !is_digit(next_char) =>
            {
                one_tone(next_tone)
            }
            (_, 0x3003, 0x3003) => 0x3002,
            (_, code, _) => code,
        };
    }
}

fn one_tone(next: u32) -> u32 {
    match next {
        0x3004 => 0x3002,
        0x3001..=0x3003 => 0x3004,
        _ => 0x3001,
    }
}

fn apply_cardinal_tones(text: &str, words: &mut Words, indices: &[usize]) {
    for &index in indices {
        let Some([(range, syllables), (following, next)]) = words.get_mut(index..index + 2) else {
            continue;
        };
        if !text[range.end..following.start]
            .chars()
            .all(char::is_whitespace)
            || text[..range.start].trim_end().ends_with('第')
            || text[following.start..].starts_with(['月', '日', '号'])
        {
            continue;
        }
        let Some(last) = syllables.last_mut().filter(|item| item.code == 0x3001) else {
            continue;
        };
        let tone = if text[range.clone()].chars().count() == 1 {
            let mut phrase = String::from("一");
            let mut fixed = None;
            for ch in text[following.start..].chars().take(2) {
                phrase.push(ch);
                if let Some(reading) = lexicon()
                    .get(phrase.as_str())
                    .and_then(|entry| entry.split_ascii_whitespace().next())
                    .and_then(syllable)
                {
                    fixed = Some(reading.code);
                }
            }
            fixed
        } else {
            None
        };
        last.code = tone.unwrap_or_else(|| one_tone(next[0].code));
    }
}

const DIGITS: [char; 10] = ['零', '一', '二', '三', '四', '五', '六', '七', '八', '九'];

fn numeral_text(number: &numbers::Number, year: bool) -> String {
    let mut whole = String::new();
    if year || number.integer.starts_with('0') || number.integer.len() > 12 {
        whole.extend(
            number
                .integer
                .bytes()
                .map(|digit| DIGITS[usize::from(digit - b'0')]),
        );
    } else {
        const PLACES: [char; 3] = ['十', '百', '千'];
        const GROUPS: [char; 2] = ['万', '亿'];
        let mut gap = false;
        for (position, group) in number.integer.as_bytes().rchunks(4).enumerate().rev() {
            if group.iter().all(|&digit| digit == b'0') {
                gap = true;
                continue;
            }
            if !whole.is_empty() && (gap || group[0] == b'0') {
                whole.push('零');
            }
            let mut pending_zero = false;
            let mut started = false;
            for (index, &digit) in group.iter().enumerate() {
                let place = group.len() - index - 1;
                if digit == b'0' {
                    pending_zero |= started;
                    continue;
                }
                if pending_zero {
                    whole.push('零');
                    pending_zero = false;
                }
                if !(whole.is_empty() && place == 1 && digit == b'1') {
                    whole.push(DIGITS[usize::from(digit - b'0')]);
                }
                if place > 0 {
                    whole.push(PLACES[place - 1]);
                }
                started = true;
            }
            if position > 0 {
                whole.push(GROUPS[position - 1]);
            }
            gap = false;
        }
    }
    let mut reading = String::new();
    if let Some(sign) = number.sign {
        reading.push(match sign {
            numbers::Sign::Plus => '正',
            numbers::Sign::Minus => '负',
        });
    }
    if number.percent {
        reading.push_str("百分之");
    }
    reading.push_str(&whole);
    if !number.fraction.is_empty() {
        reading.push('点');
        reading.extend(
            number
                .fraction
                .bytes()
                .map(|digit| DIGITS[usize::from(digit - b'0')]),
        );
    }
    reading
}

pub(super) fn format_reading(syllables: &[Syllable]) -> String {
    let mut readings = Vec::with_capacity(syllables.len());
    for syllable in syllables {
        let mut reading = syllable
            .reading
            .trim_end_matches(|ch: char| ch.is_ascii_digit())
            .to_owned();
        if (0x3001..=0x3004).contains(&syllable.code) {
            reading.push(char::from(b'0' + (syllable.code - 0x3000) as u8));
        }
        readings.push(reading);
    }
    readings.join(" ")
}

pub(super) fn edit_reading(text: &str) -> Option<Vec<Syllable>> {
    let mut syllables = Vec::new();
    for reading in text.split_whitespace() {
        let reading = reading.to_lowercase();
        let base = reading
            .strip_suffix(['0', '1', '2', '3', '4', '5'])
            .unwrap_or(&reading);
        if base.is_empty()
            || !base
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || matches!(ch, 'ü' | 'ê'))
        {
            return None;
        }
        syllables.push(syllable(&reading)?);
    }
    (!syllables.is_empty()).then_some(syllables)
}

fn syllable(reading: &str) -> Option<Syllable> {
    let base = reading
        .trim_end_matches(|ch: char| ch.is_ascii_digit())
        .replace('v', "ü");
    let tone = reading.as_bytes().last()?.wrapping_sub(b'0');
    let code = 0x3000 + if tone <= 4 { u32::from(tone) } else { 0 };
    let (initial, ending) = split(&base);
    let corrected;
    let ending = if let Some(rest) = ending.strip_prefix('y').filter(|_| initial.is_empty()) {
        corrected = match rest.chars().next()? {
            'u' => format!("ü{}", &rest[1..]),
            'i' | 'ü' => rest.to_owned(),
            _ => format!("i{rest}"),
        };
        &corrected
    } else if let Some(rest) = ending.strip_prefix('w').filter(|_| initial.is_empty()) {
        if rest.is_empty() {
            return None;
        }
        corrected = if rest.starts_with('u') {
            rest.to_owned()
        } else {
            format!("u{rest}")
        };
        &corrected
    } else if let Some(rest) = ending
        .strip_prefix('u')
        .filter(|_| matches!(initial, "j" | "q" | "x"))
    {
        corrected = format!("ü{rest}");
        &corrected
    } else {
        ending
    };
    let (nucleus, coda) = rhyme(initial, ending)?;
    let mut phones = Vec::with_capacity(3);
    if let Some(onset) = onset(initial) {
        phones.push(phone(onset, false));
    } else if ending.chars().count() > 1 {
        let glide = match ending.chars().next()? {
            'i' | 'ü' => Some("j"),
            'u' => Some("w"),
            _ => None,
        };
        if let Some(glide) = glide {
            phones.push(phone(glide, false));
        }
    }
    phones.push(phone(nucleus, true));
    if let Some(coda) = coda {
        phones.push(phone(coda, false));
    }
    Some(Syllable {
        reading: reading.to_owned(),
        code,
        phones,
    })
}

fn split(pinyin: &str) -> (&str, &str) {
    if matches!(pinyin, "m" | "n" | "ng") {
        return ("", pinyin);
    }
    for initial in ["zh", "ch", "sh"] {
        if let Some(rest) = pinyin.strip_prefix(initial) {
            return (initial, rest);
        }
    }
    if pinyin
        .as_bytes()
        .first()
        .is_some_and(|ch| b"bpmfdtnlgkhjqxrzcs".contains(ch))
    {
        return pinyin.split_at(1);
    }
    ("", pinyin)
}

fn onset(initial: &str) -> Option<&'static str> {
    Some(match initial {
        "b" => "p",
        "p" => "ph",
        "m" => "m",
        "f" => "f",
        "d" => "t",
        "t" => "th",
        "n" => "n",
        "l" => "l",
        "g" => "k",
        "k" => "kh",
        "h" => "x",
        "j" => r"ts\",
        "q" => r"ts\h",
        "x" => r"s\",
        "zh" => "ts`",
        "ch" => "ts`h",
        "sh" => "s`",
        "r" => "z`",
        "z" => "ts",
        "c" => "tsh",
        "s" => "s",
        _ => return None,
    })
}

fn rhyme(initial: &str, ending: &str) -> Option<(&'static str, Option<&'static str>)> {
    Some(match ending {
        "m" if matches!(initial, "" | "h") => ("m", None),
        "n" if initial.is_empty() => ("n", None),
        "ng" if matches!(initial, "" | "h") => ("N", None),
        "a" => ("a", None),
        "o" if matches!(initial, "b" | "p" | "m" | "f") => ("uo", None),
        "o" => ("o", None),
        "e" => ("7", None),
        "ê" => ("e", None),
        "ai" => ("a", Some(r":\i")),
        "ei" => ("e", Some(r":\i")),
        "ao" => ("AU", None),
        "ou" => ("@U", None),
        "an" => ("a", Some(":n")),
        "en" => ("@", Some(":n")),
        "ang" => ("A", Some("N")),
        "eng" => ("@", Some("N")),
        "ong" => ("U", Some("N")),
        "er" => ("a", Some(r"r\`")),
        "i" if matches!(initial, "z" | "c" | "s") => (r"i\", None),
        "i" if matches!(initial, "zh" | "ch" | "sh" | "r") => ("i`", None),
        "i" => ("i", None),
        "ia" => ("ia", None),
        "ie" => ("ie", None),
        "io" => ("o", None),
        "iao" => ("iAU", None),
        "iu" | "iou" => ("i@U", None),
        "ian" => ("iE", Some(":n")),
        "in" => ("i", Some(":n")),
        "iang" => ("iA", Some("N")),
        "ing" => ("i", Some("N")),
        "iong" => ("iU", Some("N")),
        "u" => ("u", None),
        "ua" => ("ua", None),
        "uo" => ("uo", None),
        "uai" => ("ua", Some(r":\i")),
        "ui" | "uei" => ("ue", Some(r":\i")),
        "uan" => ("ua", Some(":n")),
        "un" | "uen" => ("u@", Some(":n")),
        "uang" => ("uA", Some("N")),
        "ueng" => ("u@", Some("N")),
        "ü" => ("y", None),
        "üe" => ("yE", None),
        "üan" => ("y{", Some(":n")),
        "ün" => ("yE", Some(":n")),
        _ => return None,
    })
}
