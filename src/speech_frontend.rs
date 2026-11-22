mod english;
mod mandarin;
mod numbers;

use crate::{
    Result,
    frontend::{Tagger, UserDictionary},
    invalid,
    japanese::{self, Analysis},
    phonemes::{NONE, Phone, Syllable, Token},
};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Japanese,
    Mandarin,
    English,
}

impl Language {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "japanese" => Some(Self::Japanese),
            "mandarin" => Some(Self::Mandarin),
            "english" => Some(Self::English),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Japanese => "japanese",
            Self::Mandarin => "mandarin",
            Self::English => "english",
        }
    }

    pub fn from_token_id(id: u32) -> Option<Self> {
        [Self::Japanese, Self::Mandarin, Self::English]
            .into_iter()
            .find(|language| language.token_id() == id)
    }

    pub fn initialism_len(self, text: &str) -> Option<usize> {
        match self {
            Self::English => english::initialism_len(text),
            _ => None,
        }
    }

    pub fn honorific_len(self, text: &str) -> Option<usize> {
        match self {
            Self::English => english::honorific(text).map(|(length, _)| length),
            _ => None,
        }
    }

    pub(crate) fn model_word(self, text: &str) -> String {
        if text.is_ascii() {
            return text.to_owned();
        }
        let normalize = match self {
            Self::Japanese => false,
            Self::English => true,
            Self::Mandarin => numbers::scan(text, 0).is_some_and(|number| number.end == text.len()),
        };
        if normalize {
            text.chars()
                .map(|ch| match ch {
                    '\u{2212}' => '-',
                    '\u{2019}' => '\'',
                    _ => english::ascii_form(ch),
                })
                .collect()
        } else {
            text.to_owned()
        }
    }

    pub fn format_reading(self, syllables: &[Syllable]) -> String {
        match self {
            Self::Japanese => syllables
                .iter()
                .map(|syllable| syllable.reading.as_str())
                .collect(),
            Self::Mandarin => mandarin::format_reading(syllables),
            Self::English => english::format_reading(syllables),
        }
    }

    pub fn edit_reading(self, text: &str) -> Result<Vec<Syllable>> {
        match self {
            Self::Japanese => japanese::edit_reading(text),
            Self::Mandarin => {
                mandarin::edit_reading(text).ok_or_else(|| invalid("Invalid pinyin pronunciation"))
            }
            Self::English => {
                english::edit_reading(text).ok_or_else(|| invalid("Invalid ARPAbet pronunciation"))
            }
        }
    }

    fn token_id(self) -> u32 {
        match self {
            Self::Japanese => 1,
            Self::Mandarin => 2,
            Self::English => 3,
        }
    }

    pub fn analyze(
        self,
        text: &str,
        tagger: Option<(&Tagger, &UserDictionary)>,
    ) -> Result<Analysis> {
        if self == Self::Japanese {
            let (tagger, user) = tagger.ok_or_else(|| invalid("Japanese dictionary required"))?;
            let exclusions = tagger.normalization_exclusions(text, Some(user))?;
            let normalized = japanese::normalize_excluding(text, &exclusions)?;
            let mut analysis = japanese::analyze(&tagger.parse_with_user(&normalized, user)?)?;
            analysis.input_text = Some(normalized);
            analysis.normalization_exclusions = exclusions;
            return Ok(analysis);
        }
        if text.len() > 65_536 {
            return Err(invalid("Sentence is too long"));
        }
        let words = match self {
            Self::English => english::words(text),
            Self::Mandarin => mandarin::words(text),
            Self::Japanese => unreachable!(),
        };
        if words.len() > 4094 {
            return Err(invalid("Sentence has too many words"));
        }
        let mut tokens = vec![boundary(0x2000)];
        let mut input_ranges = vec![None];
        for (range, syllables) in words {
            let punctuation = syllables[0].phones[0].symbol == "pau";
            tokens.push(Token {
                text: text[range.clone()].to_owned(),
                language: self.token_id(),
                pos: if punctuation { 0x100b } else { 0x1017 },
                syllables,
            });
            input_ranges.push(Some(range));
        }
        tokens.push(boundary(0x2001));
        input_ranges.push(None);
        let predict = tokens
            .iter()
            .map(|token| vec![false; token.syllables.len()])
            .collect();
        Ok(Analysis {
            tokens,
            input_ranges,
            input_text: Some(text.to_owned()),
            normalization_exclusions: Default::default(),
            predict,
        })
    }
}

type Words = Vec<(Range<usize>, Vec<Syllable>)>;

fn boundary(pos: u32) -> Token {
    Token {
        text: String::new(),
        language: Language::Japanese.token_id(),
        pos,
        syllables: vec![silence("sil")],
    }
}

fn silence(symbol: &str) -> Syllable {
    Syllable {
        reading: String::new(),
        code: NONE,
        phones: vec![phone(symbol, false)],
    }
}

fn phone(symbol: &str, nucleus: bool) -> Phone {
    Phone {
        symbol: symbol.to_owned(),
        nucleus,
    }
}

fn punctuation(ch: char) -> bool {
    ",.!?;:…—，。！？、；：".contains(ch)
}
