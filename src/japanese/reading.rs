//! Inline pronunciation editing uses kana, without sentence normalization.
use crate::{
    Result, invalid,
    phonemes::{NONE, Phone, Syllable},
};

pub fn edit_reading(text: &str) -> Result<Vec<Syllable>> {
    if text.len() > 1_048_576 {
        return Err(invalid("Inline reading byte budget"));
    }
    let mut buffer = String::new();
    let mut readings = Vec::new();
    let mut previous = '\0';
    for source in text.chars() {
        let mut c = normalize_char(source);
        if vowel(c).is_none() && c != '\u{30fc}' {
            return Err(invalid(format!(
                "Unsupported inline reading character {source:?}"
            )));
        }
        if let Some(full) = full_small(c) {
            let mut joined = buffer.clone();
            joined.push(c);
            if symbols(&joined).is_some() {
                buffer.push(c);
                previous = c;
                continue;
            }
            c = full;
        }
        if !buffer.is_empty() {
            readings.push(std::mem::take(&mut buffer));
        }
        if c == '\u{30fc}' {
            c = vowel(previous).unwrap_or(c);
        }
        buffer.push(c);
        previous = c;
        if readings.len() >= 100_000 {
            return Err(invalid("Inline reading mora budget"));
        }
    }
    if !buffer.is_empty() {
        readings.push(buffer);
    }
    if readings.len() > 100_000 {
        return Err(invalid("Inline reading mora budget"));
    }
    Ok(readings
        .into_iter()
        .map(|reading| {
            let phones = symbols(&reading)
                .unwrap_or("")
                .split_whitespace()
                .map(|symbol| Phone {
                    symbol: symbol.to_owned(),
                    nucleus: super::kana::is_nucleus(symbol),
                })
                .collect();
            Syllable {
                reading,
                code: NONE,
                phones,
            }
        })
        .collect())
}

/// Retains normalized text separately from its expanded mora readings.
pub fn normalize_edit_reading(text: &str) -> Result<(String, Vec<Syllable>)> {
    if text.len() > 1_048_576 || text.contains('\0') {
        return Err(invalid("Inline reading byte or NUL budget"));
    }
    let normalized: String = text.chars().map(normalize_char).collect();
    let morae = edit_reading(&normalized)?;
    Ok((normalized, morae))
}

pub(super) fn normalize_char(c: char) -> char {
    if ('\u{3041}'..='\u{3093}').contains(&c) && !matches!(c, '\u{3090}' | '\u{3091}') {
        char::from_u32(c as u32 + 0x60).unwrap()
    } else {
        c
    }
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

pub(super) fn vowel(c: char) -> Option<char> {
    Some(match c {
        'ア' | 'カ' | 'サ' | 'タ' | 'ナ' | 'ハ' | 'マ' | 'ヤ' | 'ラ' | 'ワ' | 'ガ' | 'ザ'
        | 'ダ' | 'バ' | 'パ' | 'ァ' | 'ャ' | 'ヮ' => 'ア',
        'イ' | 'キ' | 'シ' | 'チ' | 'ニ' | 'ヒ' | 'ミ' | 'リ' | 'ギ' | 'ジ' | 'ヂ' | 'ビ'
        | 'ピ' | 'ィ' => 'イ',
        'ウ' | 'ク' | 'ス' | 'ツ' | 'ヌ' | 'フ' | 'ム' | 'ユ' | 'ル' | 'グ' | 'ヅ' | 'ズ'
        | 'ブ' | 'プ' | 'ヴ' | 'ゥ' | 'ュ' => 'ウ',
        'エ' | 'ケ' | 'セ' | 'テ' | 'ネ' | 'ヘ' | 'メ' | 'レ' | 'ゲ' | 'ゼ' | 'デ' | 'ベ'
        | 'ぺ' | 'ペ' | 'ェ' => 'エ',
        'オ' | 'ヲ' | 'コ' | 'ソ' | 'ト' | 'ノ' | 'ホ' | 'モ' | 'ヨ' | 'ロ' | 'ゴ' | 'ゾ'
        | 'ド' | 'ボ' | 'ポ' | 'ォ' | 'ョ' => 'オ',
        'ン' => 'ン',
        'ッ' => 'ッ',
        _ => return None,
    })
}

pub(super) fn symbols(reading: &str) -> Option<&'static str> {
    Some(match reading {
        "ぺ" => "p e",
        "ァ" => "a",
        "ア" => "a",
        "ィ" => "i",
        "イ" => "i",
        "イェ" => "y e",
        "ゥ" => "u",
        "ウ" => "u",
        "ウァ" => "w a",
        "ウィ" => "w i",
        "ウェ" => "w e",
        "ウォ" => "w o",
        "ウャ" => "w y a",
        "ウュ" => "w y u",
        "ェ" => "e",
        "エ" => "e",
        "ォ" => "o",
        "オ" => "o",
        "カ" => "k a",
        "ガ" => "g a",
        "キ" => "k i",
        "キィ" => "k i",
        "キェ" => "ky e",
        "キャ" => "ky a",
        "キュ" => "ky u",
        "キョ" => "ky o",
        "ギ" => "g i",
        "ギェ" => "gy e",
        "ギャ" => "gy a",
        "ギュ" => "gy u",
        "ギョ" => "gy o",
        "ク" => "k u",
        "クァ" => "kw a",
        "クィ" => "kw i",
        "クェ" => "kw e",
        "クォ" => "kw o",
        "グ" => "g u",
        "グァ" => "gw a",
        "グィ" => "gw i",
        "グェ" => "gw e",
        "グォ" => "gw o",
        "ケ" => "k e",
        "ゲ" => "g e",
        "コ" => "k o",
        "ゴ" => "g o",
        "サ" => "s a",
        "ザ" => "z a",
        "シ" => "sh i",
        "シィ" => "sh i",
        "シェ" => "sh e",
        "シャ" => "sh a",
        "シュ" => "sh u",
        "ショ" => "sh o",
        "ジ" => "j i",
        "ジェ" => "j e",
        "ジャ" => "j a",
        "ジュ" => "j u",
        "ジョ" => "j o",
        "ス" => "s u",
        "スァ" => "s a",
        "スィ" => "s i",
        "スェ" => "s y e",
        "スォ" => "s y o",
        "スャ" => "s y a",
        "ズ" => "z u",
        "ズィ" => "z i",
        "セ" => "s e",
        "ゼ" => "z e",
        "ソ" => "s o",
        "ゾ" => "z o",
        "タ" => "t a",
        "ダ" => "d a",
        "チ" => "ch i",
        "チェ" => "ch e",
        "チャ" => "ch a",
        "チュ" => "ch u",
        "チョ" => "ch o",
        "ヂ" => "j i",
        "ヂイ" => "j i",
        "ヂェ" => "j e",
        "ヂォ" => "j o",
        "ヂャ" => "j a",
        "ヂュ" => "j u",
        "ッ" => "cl",
        "ツ" => "ts u",
        "ツァ" => "ts a",
        "ツィ" => "ts i",
        "ツェ" => "ts e",
        "ツォ" => "ts o",
        "ツュ" => "ts y u",
        "ヅ" => "z u",
        "テ" => "t e",
        "ティ" => "t i",
        "テャ" => "ty a",
        "テュ" => "ty u",
        "テョ" => "ty o",
        "デ" => "d e",
        "ディ" => "d i",
        "デゥ" => "dy u",
        "デャ" => "dy a",
        "デュ" => "dy u",
        "デョ" => "dy o",
        "ト" => "t o",
        "トァ" => "t o a",
        "トゥ" => "t u",
        "トォ" => "t o",
        "ド" => "d o",
        "ドゥ" => "d u",
        "ナ" => "n a",
        "ニ" => "n i",
        "ニェ" => "ny e",
        "ニャ" => "ny a",
        "ニュ" => "ny u",
        "ニョ" => "ny o",
        "ヌ" => "n u",
        "ヌァ" => "n u a",
        "ヌィ" => "n u i",
        "ヌォ" => "n u o",
        "ネ" => "n e",
        "ノ" => "n o",
        "ハ" => "h a",
        "バ" => "b a",
        "パ" => "p a",
        "パァ" => "p a",
        "ヒ" => "h i",
        "ヒェ" => "hy e",
        "ヒャ" => "hy a",
        "ヒュ" => "hy u",
        "ヒョ" => "hy o",
        "ビ" => "b i",
        "ビィ" => "b i",
        "ビェ" => "by e",
        "ビャ" => "by a",
        "ビュ" => "by u",
        "ビョ" => "by o",
        "ピ" => "p i",
        "ピィ" => "p i",
        "ピェ" => "py e",
        "ピャ" => "py a",
        "ピュ" => "py u",
        "ピョ" => "py o",
        "フ" => "h u",
        "ファ" => "f a",
        "フィ" => "f i",
        "フェ" => "f e",
        "フォ" => "f o",
        "フュ" => "f y u",
        "フョ" => "f y o",
        "ブ" => "b u",
        "ブィ" => "b u i",
        "ブェ" => "b u e",
        "プ" => "p u",
        "プァ" => "p u a",
        "ヘ" => "h e",
        "ベ" => "b e",
        "ペ" => "p e",
        "ホ" => "h o",
        "ホァ" => "h o a",
        "ホゥ" => "h o u",
        "ボ" => "b o",
        "ボゥ" => "b o",
        "ポ" => "p o",
        "マ" => "m a",
        "ミ" => "m i",
        "ミェ" => "my e",
        "ミャ" => "my a",
        "ミュ" => "my u",
        "ミョ" => "my o",
        "ム" => "m u",
        "ムェ" => "m u e",
        "ムォ" => "m u o",
        "メ" => "m e",
        "モ" => "m o",
        "ャ" => "y a",
        "ヤ" => "y a",
        "ュ" => "y u",
        "ユ" => "y u",
        "ョ" => "y o",
        "ヨ" => "y o",
        "ラ" => "r a",
        "ラァ" => "r a",
        "リ" => "r i",
        "リェ" => "ry e",
        "リャ" => "ry a",
        "リュ" => "ry u",
        "リョ" => "ry o",
        "ル" => "r u",
        "ルァ" => "r u a",
        "ルェ" => "r u e",
        "レ" => "r e",
        "レィ" => "r i",
        "ロ" => "r o",
        "ロゥ" => "r o o",
        "ヮ" => "w a",
        "ワ" => "w a",
        "ヲ" => "w o",
        "ン" => "N",
        "ヴ" => "v u",
        "ヴァ" => "v a",
        "ヴィ" => "v i",
        "ヴィェ" => "v i e",
        "ヴゥ" => "v u",
        "ヴェ" => "v e",
        "ヴォ" => "v o",
        "ヴャ" => "v y a",
        "ヴュ" => "v y u",
        _ => return None,
    })
}
