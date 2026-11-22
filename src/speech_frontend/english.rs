mod context;

use super::{Syllable, Words, numbers, phone, punctuation, silence};
use crate::phonemes::NONE;
use std::{collections::HashMap, sync::OnceLock};

fn lexicon() -> &'static HashMap<&'static str, &'static str> {
    static LEXICON: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    LEXICON.get_or_init(|| {
        include_str!("../../assets/cmudict.dict")
            .lines()
            .filter_map(|line| line.split_once(' '))
            .collect()
    })
}

pub(super) fn ascii_form(ch: char) -> char {
    if ch == '￡' {
        '£'
    } else if ('\u{ff01}'..='\u{ff5e}').contains(&ch) {
        char::from((u32::from(ch) - 0xfee0) as u8)
    } else {
        ch
    }
}

pub(super) fn initialism_len(text: &str) -> Option<usize> {
    let mut chars = text.char_indices().peekable();
    let mut end = 0;
    let mut letters = 0;
    while let Some((_, letter)) = chars.next() {
        if !ascii_form(letter).is_ascii_alphabetic() {
            break;
        }
        let Some((index, dot)) = chars.next_if(|&(_, dot)| ascii_form(dot) == '.') else {
            break;
        };
        end = index + dot.len_utf8();
        letters += 1;
    }
    (letters >= 2
        && !text[end..].starts_with(|ch: char| {
            ascii_form(ch).is_ascii_alphanumeric() || matches!(ch, '\'' | '’')
        }))
    .then_some(end)
}

pub(super) fn honorific(text: &str) -> Option<(usize, &'static str)> {
    let (dot, mark) = text
        .char_indices()
        .take(5)
        .find(|&(_, ch)| !ascii_form(ch).is_ascii_alphabetic())?;
    if ascii_form(mark) != '.' {
        return None;
    }
    let end = dot + mark.len_utf8();
    let rest = &text[end..];
    let next = rest.trim_start_matches([' ', '\t', '\u{3000}']);
    if rest.len() == next.len() || !next.starts_with(char::is_uppercase) {
        return None;
    }
    let spelling: String = text[..dot]
        .chars()
        .map(|ch| ascii_form(ch).to_ascii_lowercase())
        .collect();
    let spoken = match spelling.as_str() {
        "dr" => {
            let following: String = next
                .chars()
                .map(ascii_form)
                .take_while(char::is_ascii_alphabetic)
                .map(|ch| ch.to_ascii_lowercase())
                .collect();
            if matches!(
                following.as_str(),
                "we" | "they" | "it" | "the" | "this" | "there" | "please" | "then" | "however"
            ) {
                return None;
            }
            "doctor"
        }
        "mr" => "mister",
        "mrs" => "missus",
        "ms" => "ms",
        "prof" => "professor",
        _ => return None,
    };
    Some((end, spoken))
}

pub(super) fn words(text: &str) -> Words {
    let mut words = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        let mut end = start + ch.len_utf8();
        let ch = ascii_form(ch);
        let mut syllables = Vec::new();
        if let Some((len, spelling)) = honorific(&text[start..]) {
            end = start + len;
            while chars.peek().is_some_and(|(index, _)| *index < end) {
                chars.next();
            }
            append_word(&mut syllables, spelling);
        } else if let Some(len) = initialism_len(&text[start..]) {
            end = start + len;
            while chars.peek().is_some_and(|(index, _)| *index < end) {
                chars.next();
            }
            let spelling: String = text[start..end]
                .chars()
                .map(|ch| ascii_form(ch).to_ascii_lowercase())
                .collect();
            append_word(&mut syllables, &spelling);
        } else if ch.is_ascii_alphabetic() {
            let mut spelling = String::from(ch.to_ascii_lowercase());
            while let Some(&(index, raw)) = chars.peek() {
                let next = match ascii_form(raw) {
                    '\'' | '’' => '\'',
                    letter if letter.is_ascii_alphabetic() => letter.to_ascii_lowercase(),
                    _ => break,
                };
                spelling.push(next);
                end = index + raw.len_utf8();
                chars.next();
            }
            let word = spelling.trim_matches('\'');
            if let Some(reading) = context::pronunciation(word, &text[..start], &text[end..]) {
                append_phones(&mut syllables, reading);
            } else {
                append_word(&mut syllables, word);
            }
        } else if let Some(currency_end) = currency(text, start, &mut syllables) {
            end = currency_end;
            while chars.peek().is_some_and(|(index, _)| *index < end) {
                chars.next();
            }
        } else if let Some(numeral) = numbers::scan(text, start) {
            end = start + numeral.end;
            if let Some(sign) = numeral.sign {
                append_sign(&mut syllables, sign);
            }
            let spelling = &numeral.integer;
            if let Ok(value) = spelling.parse::<u64>()
                && value < 1_000_000_000_000
                && (spelling.len() == 1 || !spelling.starts_with('0'))
            {
                let suffix =
                    if numeral.sign.is_none() && numeral.fraction.is_empty() && !numeral.percent {
                        ordinal_suffix(&text[end..], value)
                    } else {
                        0
                    };
                end += suffix;
                number(value, suffix != 0, &mut syllables);
            } else {
                for digit in spelling.bytes() {
                    append_word(&mut syllables, SMALL[usize::from(digit - b'0')]);
                }
            }
            if !numeral.fraction.is_empty() {
                append_word(&mut syllables, "point");
                for digit in numeral.fraction.bytes() {
                    append_word(&mut syllables, SMALL[usize::from(digit - b'0')]);
                }
            }
            if numeral.percent {
                append_word(&mut syllables, "percent");
            }
            while chars.peek().is_some_and(|(index, _)| *index < end) {
                chars.next();
            }
        } else if punctuation(ch) {
            syllables.push(silence("pau"));
        }
        if !syllables.is_empty() {
            words.push((start..end, syllables));
        }
    }
    words
}

fn currency(text: &str, start: usize, out: &mut Vec<Syllable>) -> Option<usize> {
    let first = text[start..].chars().next()?;
    let sign = numbers::sign(first);
    let symbol = start + sign.map_or(0, |_| first.len_utf8());
    let mark = text[symbol..].chars().next()?;
    let (unit, plural, minor_unit, minor_plural) = match ascii_form(mark) {
        '$' => ("dollar", "dollars", "cent", "cents"),
        '€' => ("euro", "euros", "cent", "cents"),
        '£' => ("pound", "pounds", "penny", "pence"),
        _ => return None,
    };
    let amount_at = symbol + mark.len_utf8();
    let amount = numbers::scan(text, amount_at)?;
    if amount.percent || (sign.is_some() && amount.sign.is_some()) {
        return None;
    }
    let value = amount
        .integer
        .parse::<u64>()
        .ok()
        .filter(|n| *n < 1_000_000_000_000)?;
    if let Some(sign) = sign.or(amount.sign) {
        append_sign(out, sign);
    }
    let fraction = amount.fraction.as_bytes();
    if fraction.len() > 2 {
        number(value, false, out);
        append_word(out, "point");
        for &digit in fraction {
            append_word(out, SMALL[usize::from(digit - b'0')]);
        }
        append_word(out, plural);
    } else {
        let tenths = fraction.first().map_or(0, |digit| digit - b'0');
        let hundredths = fraction.get(1).map_or(0, |digit| digit - b'0');
        let minor = u64::from(tenths * 10 + hundredths);
        if value != 0 || minor == 0 {
            number(value, false, out);
            append_word(out, if value == 1 { unit } else { plural });
        }
        if minor != 0 {
            if value != 0 {
                append_word(out, "and");
            }
            number(minor, false, out);
            append_word(out, if minor == 1 { minor_unit } else { minor_plural });
        }
    }
    Some(amount_at + amount.end)
}

fn append_sign(out: &mut Vec<Syllable>, sign: numbers::Sign) {
    append_word(
        out,
        match sign {
            numbers::Sign::Plus => "plus",
            numbers::Sign::Minus => "minus",
        },
    );
}

fn append_word(out: &mut Vec<Syllable>, spelling: &str) {
    let pronunciation = match spelling {
        "e.g." => Some("IY2 JH IY1"),
        _ => lexicon().get(spelling).copied(),
    };
    if let Some(phones) = pronunciation {
        append_phones(out, phones);
    } else {
        let letters = spelling
            .bytes()
            .filter(u8::is_ascii_lowercase)
            .filter_map(|ch| {
                let letter = [ch];
                lexicon()
                    .get(std::str::from_utf8(&letter).unwrap())
                    .copied()
            })
            .collect::<Vec<_>>()
            .join(" ");
        append_phones(out, &letters);
    }
}

pub(super) fn format_reading(syllables: &[Syllable]) -> String {
    let mut words = Vec::new();
    for syllable in syllables {
        let stress = syllable.code.saturating_sub(0x4000).min(2) as u8;
        for item in &syllable.phones {
            let mut value = if item.symbol == "ax" && item.nucleus && stress == 0 {
                "AH".to_owned()
            } else {
                item.symbol.to_ascii_uppercase()
            };
            if item.nucleus {
                value.push(char::from(b'0' + stress));
            }
            words.push(value);
        }
    }
    words.join(" ")
}

pub(super) fn edit_reading(text: &str) -> Option<Vec<Syllable>> {
    const VOWELS: &[&str] = &[
        "aa", "ae", "ah", "ao", "aw", "ax", "ay", "eh", "er", "ey", "ih", "iy", "ow", "oy", "uh",
        "uw",
    ];
    const CONSONANTS: &[&str] = &[
        "b", "ch", "d", "dh", "f", "g", "hh", "j", "jh", "k", "l", "m", "n", "ng", "p", "r", "s",
        "sh", "t", "th", "v", "w", "y", "z", "zh",
    ];
    let text = text.to_ascii_lowercase();
    let mut has_vowel = false;
    for phone in text.split_ascii_whitespace() {
        if let Some(vowel) = phone.strip_suffix(['0', '1', '2']) {
            if !VOWELS.contains(&vowel) {
                return None;
            }
            has_vowel = true;
        } else if !CONSONANTS.contains(&phone) {
            return None;
        }
    }
    if !has_vowel {
        return None;
    }
    let mut syllables = Vec::new();
    append_phones(&mut syllables, &text);
    Some(syllables)
}

fn append_phones(out: &mut Vec<Syllable>, pronunciation: &str) {
    let first = out.len();
    let mut onset = Vec::new();
    for phoneme in pronunciation.split_ascii_whitespace() {
        let symbol = phoneme.to_ascii_lowercase();
        if let Some(stress @ b'0'..=b'2') = symbol.bytes().last() {
            let vowel = &symbol[..symbol.len() - 1];
            let vowel = if vowel == "ah" && stress == b'0' {
                "ax"
            } else {
                vowel
            };
            onset.push(phone(vowel, true));
            out.push(Syllable {
                reading: vowel.to_owned(),
                code: 0x4000 + u32::from(stress - b'0'),
                phones: std::mem::take(&mut onset),
            });
        } else {
            onset.push(phone(&symbol, false));
        }
    }
    if !onset.is_empty() {
        if out.len() > first {
            out.last_mut().unwrap().phones.extend(onset);
        } else {
            out.push(Syllable {
                reading: String::new(),
                code: NONE,
                phones: onset,
            });
        }
    }
}

const SMALL: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 8] = [
    "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];
const SMALL_ORDINALS: [&str; 20] = [
    "zeroth",
    "first",
    "second",
    "third",
    "fourth",
    "fifth",
    "sixth",
    "seventh",
    "eighth",
    "ninth",
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
];
const TENS_ORDINALS: [&str; 8] = [
    "twentieth",
    "thirtieth",
    "fortieth",
    "fiftieth",
    "sixtieth",
    "seventieth",
    "eightieth",
    "ninetieth",
];

fn ordinal_suffix(text: &str, value: u64) -> usize {
    let expected = if matches!(value % 100, 11..=13) {
        ['t', 'h']
    } else {
        match value % 10 {
            1 => ['s', 't'],
            2 => ['n', 'd'],
            3 => ['r', 'd'],
            _ => ['t', 'h'],
        }
    };
    let mut chars = text.chars();
    let mut bytes = 0;
    for expected in expected {
        let Some(ch) = chars.next() else { return 0 };
        if !ascii_form(ch).eq_ignore_ascii_case(&expected) {
            return 0;
        }
        bytes += ch.len_utf8();
    }
    if chars
        .next()
        .is_some_and(|ch| ch.is_alphanumeric() || ch == '_')
    {
        0
    } else {
        bytes
    }
}

fn number(mut value: u64, ordinal: bool, out: &mut Vec<Syllable>) {
    let small = if ordinal { &SMALL_ORDINALS } else { &SMALL };
    if value < 20 {
        append_word(out, small[value as usize]);
        return;
    }
    for (scale, cardinal, ordinal_name) in [
        (1_000_000_000, "billion", "billionth"),
        (1_000_000, "million", "millionth"),
        (1_000, "thousand", "thousandth"),
        (100, "hundred", "hundredth"),
    ] {
        if value >= scale {
            number(value / scale, false, out);
            value %= scale;
            if ordinal && value == 0 {
                append_word(out, ordinal_name);
                return;
            }
            append_word(out, cardinal);
        }
    }
    if value >= 20 {
        let ten = (value / 10 - 2) as usize;
        value %= 10;
        let word = if ordinal && value == 0 {
            TENS_ORDINALS[ten]
        } else {
            TENS[ten]
        };
        append_word(out, word);
    }
    if value > 0 {
        append_word(out, small[value as usize]);
    }
}
