//! Japanese dictionary morphology to the explicit token inference interface.
//! No installed files, user dictionaries or processes are discovered here.
mod accent;
mod kana;
mod normalize;
mod numbers;
mod origin;
mod prefix;
mod prefix_data;
mod reading;
mod ssml;
mod user_words;

pub use kana::{is_nucleus, morae, phones};
pub use normalize::{
    normalize, normalize_excluding, normalize_with_origins, normalize_with_origins_excluding,
};
pub use numbers::expand as expand_numbers;
pub use origin::NormalizedText;
pub use reading::{edit_reading, normalize_edit_reading};
pub use ssml::{SsmlMora, SsmlReading, ssml_reading};
pub use user_words::{Pronunciation, analyze_text};

use crate::{
    Result,
    frontend::Morph,
    invalid,
    phonemes::{HIGH, LOW, NONE, Phone, Syllable, Token},
};

struct Word {
    input_range: Option<std::ops::Range<usize>>,
    text: String,
    pos: String,
    detail: String,
    pronunciation: String,
    accent_type: String,
    connection_type: String,
    modification: String,
    morae: Vec<String>,
    marks: Vec<bool>,
    accent: i32,
    explicit: Option<String>,
    connection: accent::Connection,
    mod_kind: String,
    mod_value: i32,
    lexical_id: u64,
    fixed: bool,
}

impl Word {
    fn new(text: &str, pos: &str, detail: &str, reading: &str) -> Result<Self> {
        let mut word = Self {
            input_range: None,
            text: text.into(),
            pos: pos.into(),
            detail: detail.into(),
            pronunciation: String::new(),
            accent_type: String::new(),
            connection_type: String::new(),
            modification: String::new(),
            morae: Vec::new(),
            marks: Vec::new(),
            accent: 0,
            explicit: None,
            connection: accent::Connection::default(),
            mod_kind: String::new(),
            mod_value: 0,
            lexical_id: 0,
            fixed: false,
        };
        word.set_reading(reading)?;
        Ok(word)
    }

    fn boundary(&self) -> bool {
        self.pos == "BOS/EOS"
    }

    fn set_reading(&mut self, reading: &str) -> Result<()> {
        self.morae = morae(reading)?;
        self.marks = vec![false; self.morae.len()];
        self.pronunciation = reading.into();
        Ok(())
    }

    fn set_accent(&mut self, value: i32) {
        self.marks = accent::pattern(self.morae.len(), value);
    }
}

/// Reads a dictionary CSV record, including quoted commas and escaped quotes.
/// The bundled Japanese schema has seven fields; other schemas are explicit
/// errors instead of guessed UniDic/IPADIC column positions.
fn fields(record: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = record.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                result.push(std::mem::take(&mut field));
            }
            _ => field.push(c),
        }
    }
    if quoted {
        return Err(invalid("unterminated quote in Japanese dictionary feature"));
    }
    result.push(field);
    Ok(result)
}

fn punctuation(pos: &str) -> bool {
    matches!(pos, "補助記号" | "記号" | "空白")
}

fn word(morph: &Morph) -> Result<Word> {
    if morph.surface.len() > 1_048_576 || morph.feature.len() > 1_048_576 {
        return Err(invalid("Japanese morphology field byte budget"));
    }
    let f = fields(&morph.feature)?;
    if f.len() < 2 {
        return Err(invalid("Japanese morphology requires POS and subcategory"));
    }
    if f.len() != 7 && !morph.unknown {
        return Err(invalid(format!(
            "unsupported Japanese dictionary schema ({} fields)",
            f.len()
        )));
    }
    let pause = punctuation(&f[0]);
    let pron = if f.len() == 7 && f[2] != "*" && !f[2].is_empty() {
        &f[2]
    } else if pause {
        ""
    } else if morph.unknown {
        &morph.surface
    } else {
        return Err(invalid(format!(
            "dictionary entry {:?} has no pronunciation",
            morph.surface
        )));
    };
    let mut result = Word::new(&morph.surface, &f[0], &f[1], pron)?;
    result.input_range = Some(morph.start..morph.end);
    if result.morae.is_empty() && !pause {
        return Err(invalid(format!("no spoken morae for {:?}", morph.surface)));
    }
    if f.len() == 7 {
        result.accent_type.clone_from(&f[3]);
        result.connection_type.clone_from(&f[4]);
        result.modification.clone_from(&f[5]);
        if f[6] != "*" && !f[6].is_empty() {
            result.lexical_id = f[6]
                .parse::<u64>()
                .map_err(|_| invalid("Japanese dictionary lexical identifier is not u64"))?;
        }
    }
    Ok(result)
}

fn pos(kind: &str) -> Result<u32> {
    const NAMES: [&str; 16] = [
        "名詞",
        "代名詞",
        "副詞",
        "接続詞",
        "感動詞",
        "動詞",
        "形容詞",
        "助動詞",
        "助詞",
        "接頭辞",
        "接尾辞",
        "補助記号",
        "空白",
        "記号",
        "形状詞",
        "連体詞",
    ];
    NAMES
        .iter()
        .position(|name| *name == kind)
        .map(|i| 0x1000 + i as u32)
        .ok_or_else(|| invalid(format!("unsupported Japanese part of speech {kind:?}")))
}

fn fix_readings(words: &mut [Word]) -> Result<()> {
    for i in 0..words.len() {
        let previous = i.checked_sub(1).map(|j| words[j].text.clone());
        let next = words
            .get(i + 1)
            .map(|w| (w.text.clone(), w.pronunciation.clone(), w.pos.clone()));
        let w = &mut words[i];
        let next_text = next.as_ref().map(|n| n.0.as_str());
        if w.text == "私" && w.pronunciation == "ワタクシ" {
            w.set_reading("ワタシ")?;
            w.set_accent(0);
        } else if w.text == "日本" && next_text == Some("語") {
            w.set_reading("ニホン")?;
            w.set_accent(0);
        } else if matches!(previous.as_deref(), Some("父" | "母"))
            && w.text == "上"
            && w.pronunciation == "ジョー"
        {
            w.set_reading("ウエ")?;
            w.set_accent(0);
        } else if matches!(next_text, Some("さん" | "さま" | "様")) {
            let pron = match (w.text.as_str(), w.pronunciation.as_str()) {
                ("父", "チチ") => Some("トー"),
                ("母", "ハハ") => Some("カー"),
                _ => None,
            };
            if let Some(pron) = pron {
                w.set_reading(pron)?;
                w.set_accent(0);
            }
        }
        if matches!(w.text.as_str(), "二" | "ニ")
            && next.as_ref().is_some_and(|n| n.0 == "宮" && n.1 == "グー")
        {
            w.set_reading("ニノ")?;
            w.set_accent(0);
            words[i + 1].set_reading("ミヤ")?;
            words[i + 1].set_accent(1);
        }
        if words[i].text == "何" {
            if next
                .as_ref()
                .is_none_or(|n| matches!(n.2.as_str(), "BOS/EOS" | "補助記号"))
            {
                words[i].set_reading("ナニ")?;
                words[i].set_accent(1);
            } else if matches!(next_text, Some("も" | "モ")) {
                words[i].set_reading("ナニ")?;
                words[i].set_accent(0);
                words[i + 1].set_reading("モ")?;
                words[i + 1].set_accent(1);
            } else if matches!(next_text, Some("ガ" | "が" | "を" | "ヲ" | "よ" | "ヨ")) {
                words[i].set_reading("ナニ")?;
                words[i].set_accent(1);
                words[i + 1].set_accent(0);
            }
        }
    }
    Ok(())
}

fn boundary(kind: u32) -> Token {
    Token {
        text: String::new(),
        language: 1,
        pos: kind,
        syllables: vec![Syllable {
            reading: String::new(),
            code: NONE,
            phones: vec![Phone {
                symbol: "sil".into(),
                nucleus: false,
            }],
        }],
    }
}

/// Converts morphology from an already-normalized Japanese sentence into a
/// complete BOS/EOS token tree. Unknown kana and ASCII letter names are spoken;
/// unknown kanji, unsupported schemas/readings and missing pronunciations fail.
pub fn convert(morphs: &[Morph]) -> Result<Vec<Token>> {
    Ok(analyze(morphs)?.tokens)
}

/// Native accent-prediction eligibility accompanies the token tree. This is
/// separate from `Token` so callers can preserve manual/dictionary accents
/// without changing the explicit token interchange format.
pub struct Analysis {
    pub tokens: Vec<Token>,
    pub input_ranges: Vec<Option<std::ops::Range<usize>>>,
    pub input_text: Option<String>,
    /// Raw user-dictionary characters excluded during this analysis normalization.
    pub normalization_exclusions: std::collections::BTreeSet<char>,
    /// One boolean per syllable, grouped by token; false for BOS/EOS and pauses.
    pub predict: Vec<Vec<bool>>,
}

pub fn analyze(morphs: &[Morph]) -> Result<Analysis> {
    if morphs.len() > 4094 {
        return Err(invalid("Japanese token budget"));
    }
    if morphs
        .iter()
        .map(|m| m.surface.len() + m.feature.len())
        .sum::<usize>()
        > 16_777_216
    {
        return Err(invalid("Japanese morphology byte budget"));
    }
    finish(morphs.iter().map(word).collect::<Result<Vec<_>>>()?)
}

fn finish(mut words: Vec<Word>) -> Result<Analysis> {
    if words.len() > 4094 {
        return Err(invalid("Japanese token budget"));
    }
    words.insert(0, Word::new("", "BOS/EOS", "", "")?);
    words.push(Word::new("", "BOS/EOS", "", "")?);
    if words.iter().map(|w| w.morae.len()).sum::<usize>() > 100_000 {
        return Err(invalid("Japanese sentence mora budget"));
    }
    accent::assign(&mut words)?;
    numbers::adjust_digits(&mut words)?;
    fix_readings(&mut words)?;
    numbers::adjust_counters(&mut words)?;
    let mut tokens = vec![boundary(0x2000)];
    let mut input_ranges = vec![None];
    let mut predict = vec![vec![false]];
    for w in words.into_iter().filter(|w| !w.boundary()) {
        input_ranges.push(w.input_range.clone());
        let pos = pos(&w.pos)?;
        let eligible = w.lexical_id >> 32 != 0 && !w.fixed;
        let syllables = if w.morae.is_empty() {
            vec![Syllable {
                reading: String::new(),
                code: NONE,
                phones: vec![Phone {
                    symbol: "pau".into(),
                    nucleus: false,
                }],
            }]
        } else {
            w.morae
                .into_iter()
                .zip(w.marks)
                .map(|(reading, high)| {
                    Ok(Syllable {
                        phones: phones(&reading)?,
                        reading,
                        code: if high { HIGH } else { LOW },
                    })
                })
                .collect::<Result<Vec<_>>>()?
        };
        predict.push(vec![
            eligible && !w.pronunciation.is_empty();
            syllables.len()
        ]);
        tokens.push(Token {
            text: w.text,
            language: 1,
            pos,
            syllables,
        });
    }
    tokens.push(boundary(0x2001));
    input_ranges.push(None);
    predict.push(vec![false]);
    Ok(Analysis {
        tokens,
        input_ranges,
        input_text: None,
        normalization_exclusions: Default::default(),
        predict,
    })
}
