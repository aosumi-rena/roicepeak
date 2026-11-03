//! Japanese mora segmentation and explicit model phone spelling.
use crate::{Result, invalid, phonemes::Phone};

/// Japanese model-phone nuclei, distinct from newly constructed editor flags.
pub fn is_nucleus(symbol: &str) -> bool {
    matches!(symbol, "a" | "i" | "u" | "e" | "o" | "N")
}

fn letter(c: char) -> Option<&'static str> {
    Some(match c.to_ascii_lowercase() {
        'a' => "エイ",
        'b' => "ビイ",
        'c' => "シイ",
        'd' => "ディイ",
        'e' => "イイ",
        'f' => "エフ",
        'g' => "ジイ",
        'h' => "エイチ",
        'i' => "アイ",
        'j' => "ジェイ",
        'k' => "ケイ",
        'l' => "エル",
        'm' => "エム",
        'n' => "エヌ",
        'o' => "オオ",
        'p' => "ピイ",
        'q' => "キュウ",
        'r' => "アアル",
        's' => "エス",
        't' => "ティイ",
        'u' => "ユウ",
        'v' => "ブイ",
        'w' => "ダブリュウ",
        'x' => "エックス",
        'y' => "ワイ",
        'z' => "ゼット",
        _ => return None,
    })
}

fn full_small(c: char) -> Option<char> {
    Some(match c {
        'ァ' => 'ア',
        'ィ' => 'イ',
        'ゥ' => 'ウ',
        'ェ' => 'エ',
        'ォ' => 'オ',
        'ャ' => 'ヤ',
        'ュ' => 'ユ',
        'ョ' => 'ヨ',
        'ヮ' => 'ワ',
        _ => return None,
    })
}

fn vowel(c: char) -> Option<char> {
    for (v, row) in [
        ('ア', "ァアカガサザタダナハバパマャヤラヮワ"),
        ('イ', "ィイキギシジチヂニヒビピミリ"),
        ('ウ', "ゥウクグスズツヅヌフブプムュユルヴ"),
        ('エ', "ェエケゲセゼテデネヘベペメレ"),
        ('オ', "ォオコゴソゾトドノホボポモョヨロヲ"),
        ('ン', "ン"),
        ('ッ', "ッ"),
    ] {
        if row.contains(c) {
            return Some(v);
        }
    }
    None
}

/// Normalizes a pronunciation, not an orthographic sentence. Unsupported
/// pronunciation characters are errors, so unknown text cannot disappear.
pub fn morae(pronunciation: &str) -> Result<Vec<String>> {
    if pronunciation.len() > 1_048_576 {
        return Err(invalid("Japanese pronunciation byte budget"));
    }
    let mut kana = String::new();
    for c in pronunciation.chars() {
        if c.is_ascii_alphabetic() {
            kana.push_str(letter(c).unwrap());
        } else if ('ぁ'..='ん').contains(&c) && !matches!(c, 'ゐ' | 'ゑ') {
            kana.push(char::from_u32(c as u32 + 0x60).unwrap());
        } else if vowel(c).is_some() || c == 'ー' {
            kana.push(c);
        } else {
            return Err(invalid(format!(
                "unsupported Japanese pronunciation character {c:?}"
            )));
        }
    }
    let mut result: Vec<String> = Vec::new();
    for c in kana.chars() {
        if c == 'ー' {
            let preceding = result
                .last()
                .and_then(|m| m.chars().last())
                .and_then(vowel)
                .ok_or_else(|| invalid("long vowel without preceding mora"))?;
            result.push(preceding.to_string());
        } else if let Some(standalone) = full_small(c) {
            if let Some(previous) = result.last_mut() {
                let joined = format!("{previous}{c}");
                if symbols(&joined).is_some() {
                    previous.push(c);
                    continue;
                }
            }
            result.push(standalone.to_string());
        } else {
            result.push(c.to_string());
        }
    }
    if result.len() > 100_000 {
        return Err(invalid("Japanese mora budget"));
    }
    for mora in &result {
        if symbols(mora).is_none() {
            return Err(invalid(format!("unsupported Japanese mora {mora:?}")));
        }
    }
    Ok(result)
}

/// Kana/phone correspondences are linguistic data. Regular rows are generated
/// from their onset and vowel; contracted and foreign morae are explicit.
fn symbols(mora: &str) -> Option<String> {
    for (onset, row) in [
        ("", "アイウエオ"),
        ("k", "カキクケコ"),
        ("g", "ガギグゲゴ"),
        ("s", "サ□スセソ"),
        ("z", "ザ□ズゼゾ"),
        ("t", "タ□□テト"),
        ("d", "ダ□□デド"),
        ("n", "ナニヌネノ"),
        ("h", "ハヒフヘホ"),
        ("b", "バビブベボ"),
        ("p", "パピプペポ"),
        ("m", "マミムメモ"),
        ("r", "ラリルレロ"),
    ] {
        if mora.chars().count() == 1 {
            let c = mora.chars().next()?;
            if c != '□' {
                if let Some(i) = row.chars().position(|k| k == c) {
                    let v = ["a", "i", "u", "e", "o"][i];
                    return Some(if onset.is_empty() {
                        v.into()
                    } else {
                        format!("{onset} {v}")
                    });
                }
            }
        }
    }
    let basic = match mora {
        "シ" => "sh i",
        "ジ" | "ヂ" => "j i",
        "チ" => "ch i",
        "ツ" => "ts u",
        "ヅ" => "z u",
        "ヤ" => "y a",
        "ユ" => "y u",
        "ヨ" => "y o",
        "ワ" => "w a",
        "ヲ" => "w o",
        "ン" => "N",
        "ッ" => "cl",
        "ヴ" => "v u",
        _ => "",
    };
    if !basic.is_empty() {
        return Some(basic.into());
    }
    let chars: Vec<_> = mora.chars().collect();
    if chars.len() == 2 {
        let onset = match chars[0] {
            'キ' => "ky",
            'ギ' => "gy",
            'シ' => "sh",
            'ジ' | 'ヂ' => "j",
            'チ' => "ch",
            'ニ' => "ny",
            'ヒ' => "hy",
            'ビ' => "by",
            'ピ' => "py",
            'ミ' => "my",
            'リ' => "ry",
            'テ' => "ty",
            'デ' => "dy",
            _ => "",
        };
        let tail = match chars[1] {
            'ャ' => "a",
            'ュ' => "u",
            'ョ' => "o",
            'ェ' => "e",
            _ => "",
        };
        // テ/デ + small エ are separate morae; the other listed onsets support it.
        if !onset.is_empty()
            && !tail.is_empty()
            && !(matches!(chars[0], 'テ' | 'デ') && chars[1] == 'ェ')
            && !(chars[0] == 'ヂ' && chars[1] == 'ョ')
        {
            return Some(format!("{onset} {tail}"));
        }
        let onset = match chars[0] {
            'ク' => "kw",
            'グ' => "gw",
            'フ' => "f",
            'ヴ' => "v",
            _ => "",
        };
        let tail = match chars[1] {
            'ァ' => "a",
            'ィ' => "i",
            'ェ' => "e",
            'ォ' => "o",
            _ => "",
        };
        if !onset.is_empty() && !tail.is_empty() {
            return Some(format!("{onset} {tail}"));
        }
    }
    Some(
        match mora {
            "イェ" => "y e",
            "ウァ" => "w a",
            "ウィ" => "w i",
            "ウェ" => "w e",
            "ウォ" => "w o",
            "ウャ" => "w y a",
            "ウュ" => "w y u",
            "キィ" => "k i",
            "シィ" => "sh i",
            "スァ" => "s a",
            "スィ" => "s i",
            "スェ" => "s y e",
            "スォ" => "s y o",
            "スャ" => "s y a",
            "ズィ" => "z i",
            "ヂイ" => "j i",
            "ヂォ" => "j o",
            "ツァ" => "ts a",
            "ツィ" => "ts i",
            "ツェ" => "ts e",
            "ツォ" => "ts o",
            "ツュ" => "ts y u",
            "ティ" => "t i",
            "ディ" => "d i",
            "デゥ" => "dy u",
            "トァ" => "t o a",
            "トゥ" => "t u",
            "トォ" => "t o",
            "ドゥ" => "d u",
            "ヌァ" => "n u a",
            "ヌィ" => "n u i",
            "ヌォ" => "n u o",
            "パァ" => "p a",
            "ビィ" => "b i",
            "ピィ" => "p i",
            "フュ" => "f y u",
            "フョ" => "f y o",
            "ブィ" => "b u i",
            "ブェ" => "b u e",
            "プァ" => "p u a",
            "ホァ" => "h o a",
            "ホゥ" => "h o u",
            "ボゥ" => "b o",
            "ムェ" => "m u e",
            "ムォ" => "m u o",
            "ラァ" => "r a",
            "ルァ" => "r u a",
            "ルェ" => "r u e",
            "レィ" => "r i",
            "ロゥ" => "r o o",
            "ヴゥ" => "v u",
            "ヴャ" => "v y a",
            "ヴュ" => "v y u",
            "ヴィェ" => "v i e",
            _ => return None,
        }
        .into(),
    )
}

pub fn phones(mora: &str) -> Result<Vec<Phone>> {
    let symbols =
        symbols(mora).ok_or_else(|| invalid(format!("unsupported Japanese mora {mora:?}")))?;
    Ok(symbols
        .split_whitespace()
        .map(|symbol| Phone {
            symbol: symbol.into(),
            nucleus: is_nucleus(symbol),
        })
        .collect())
}
