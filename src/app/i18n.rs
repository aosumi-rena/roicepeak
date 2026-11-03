//! ROICEPEAK locale catalog. Native translations remain an optional local pack.
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::BTreeMap, io::Read, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Locale {
    #[serde(rename = "en-us")]
    English,
    #[serde(rename = "zh-cn")]
    SimplifiedChinese,
    #[serde(rename = "zh-tw")]
    TraditionalChinese,
    #[default]
    #[serde(rename = "ja-jp")]
    Japanese,
    #[serde(rename = "ko-kr")]
    Korean,
    #[serde(rename = "de-de")]
    German,
    #[serde(rename = "es-la")]
    Spanish,
    #[serde(rename = "fr-fr")]
    French,
    #[serde(rename = "pt-br")]
    Portuguese,
    #[serde(rename = "ru-ru")]
    Russian,
    #[serde(rename = "vi-vn")]
    Vietnamese,
}

impl Locale {
    pub const ALL: [Self; 11] = [
        Self::English,
        Self::SimplifiedChinese,
        Self::TraditionalChinese,
        Self::Japanese,
        Self::Korean,
        Self::German,
        Self::Spanish,
        Self::French,
        Self::Portuguese,
        Self::Russian,
        Self::Vietnamese,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::English => "en-us",
            Self::SimplifiedChinese => "zh-cn",
            Self::TraditionalChinese => "zh-tw",
            Self::Japanese => "ja-jp",
            Self::Korean => "ko-kr",
            Self::German => "de-de",
            Self::Spanish => "es-la",
            Self::French => "fr-fr",
            Self::Portuguese => "pt-br",
            Self::Russian => "ru-ru",
            Self::Vietnamese => "vi-vn",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
            Self::TraditionalChinese => "繁體中文",
            Self::Japanese => "日本語",
            Self::Korean => "한국어",
            Self::German => "Deutsch",
            Self::Spanish => "Español",
            Self::French => "Français",
            Self::Portuguese => "Português",
            Self::Russian => "Русский",
            Self::Vietnamese => "Tiếng Việt",
        }
    }
}

#[derive(Deserialize)]
struct Catalog {
    aliases: BTreeMap<String, String>,
    locales: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Deserialize)]
struct NativePair {
    key: String,
    text: String,
}

#[derive(Default)]
struct NativeCatalog {
    pairs: usize,
    exact: BTreeMap<String, String>,
    translated: BTreeMap<String, String>,
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();
static NATIVE: OnceLock<NativeCatalog> = OnceLock::new();
static NATIVE_JAPANESE: OnceLock<NativeCatalog> = OnceLock::new();
static NATIVE_ENGLISH: OnceLock<NativeCatalog> = OnceLock::new();

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/locales/catalogue.json"))
            .expect("embedded ROICEPEAK locale catalog")
    })
}

fn canonical(key: &str) -> String {
    key.trim().trim_end_matches(['.', '…']).to_lowercase()
}

fn load_native(filename: &str) -> NativeCatalog {
    let load = || -> Option<Vec<NativePair>> {
        let path = std::env::current_exe()
            .ok()?
            .parent()?
            .join("resources/locales")
            .join(filename);
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(524_289)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.len() > 524_288 {
            return None;
        }
        let pairs: Vec<NativePair> = serde_json::from_slice(&bytes).ok()?;
        (pairs.len() <= 4096
            && pairs
                .iter()
                .all(|pair| pair.key.len() <= 8192 && pair.text.len() <= 8192))
        .then_some(pairs)
    };
    let Some(pairs) = load() else {
        return NativeCatalog::default();
    };
    let pair_count = pairs.len();
    let exact: BTreeMap<_, _> = pairs
        .into_iter()
        .map(|pair| (pair.key, pair.text))
        .collect();
    let normalized: BTreeMap<_, _> = exact
        .iter()
        .map(|(key, text)| (canonical(key), text))
        .collect();
    let mut translated = exact.clone();
    // Resolve case/ellipsis variants once, never allocate lookup keys per frame.
    for entries in catalog().locales.values() {
        for key in entries.keys() {
            if !translated.contains_key(key) {
                if let Some(text) = normalized.get(&canonical(key)) {
                    translated.insert(key.clone(), (*text).clone());
                }
            }
        }
    }
    NativeCatalog {
        pairs: pair_count,
        exact,
        translated,
    }
}
fn native() -> &'static NativeCatalog {
    NATIVE.get_or_init(|| load_native("zh-cn-native.json"))
}
fn native_japanese() -> &'static NativeCatalog {
    NATIVE_JAPANESE.get_or_init(|| load_native("ja-jp-native.json"))
}
fn native_english() -> &'static NativeCatalog {
    NATIVE_ENGLISH.get_or_init(|| load_native("en-us-native.json"))
}
/// Narrator IDs are localization keys, not necessarily literal display names.
/// Optional native packs stay local; custom locales use their own exact key or
/// the English pack. Unmapped Japanese literal names are preserved as supplied.
pub fn narrator_name(locale: Locale, key: &str) -> &str {
    if locale == Locale::Japanese {
        return native_japanese().exact.get(key).map_or(key, String::as_str);
    }
    if locale == Locale::SimplifiedChinese {
        if let Some(name) = native().exact.get(key) {
            return name;
        }
    }
    if locale != Locale::English {
        if let Some(name) = catalog()
            .locales
            .get(locale.key())
            .and_then(|entries| entries.get(key))
        {
            return name;
        }
    }
    native_english().exact.get(key).map_or(key, String::as_str)
}

pub fn text(locale: Locale, source: &str) -> &str {
    let data = catalog();
    if locale == Locale::Japanese && data.aliases.contains_key(source) {
        return source;
    }
    let key = data.aliases.get(source).map_or(source, String::as_str);
    if locale == Locale::English {
        return key;
    }
    if locale == Locale::SimplifiedChinese {
        if let Some(value) = native().translated.get(key) {
            return value;
        }
    }
    data.locales
        .get(locale.key())
        .and_then(|entries| entries.get(key))
        .map_or(key, String::as_str)
}

/// Translate a completed status/error template without changing inserted values.
pub fn message(locale: Locale, rendered: &str) -> Cow<'_, str> {
    let exact = text(locale, rendered);
    if exact != rendered {
        return Cow::Borrowed(exact);
    }
    let Some(entries) = catalog().locales.get(locale.key()) else {
        return Cow::Borrowed(rendered);
    };
    for source in entries.keys().filter(|source| source.contains("{}")) {
        let parts: Vec<_> = source.split("{}").collect();
        let Some(mut rest) = rendered.strip_prefix(parts[0]) else {
            continue;
        };
        let mut arguments = Vec::new();
        let mut matched = true;
        for (index, part) in parts[1..].iter().enumerate() {
            if part.is_empty() && index + 2 == parts.len() {
                arguments.push(rest);
                rest = "";
            } else if let Some(at) = rest.find(part) {
                arguments.push(&rest[..at]);
                rest = &rest[at + part.len()..];
            } else {
                matched = false;
                break;
            }
        }
        if matched && rest.is_empty() {
            return Cow::Owned(format(locale, source, &arguments));
        }
    }
    Cow::Borrowed(rendered)
}

pub fn tr<'a>(locale: Locale, japanese: &'a str, english: &'a str) -> &'a str {
    if locale == Locale::Japanese && !japanese.is_empty() {
        japanese
    } else {
        text(locale, english)
    }
}

/// Insert arguments after translation, retaining their original order.
pub fn format(locale: Locale, source: &str, arguments: &[&str]) -> String {
    let mut output = String::new();
    let mut pieces = text(locale, source).split("{}");
    output.push_str(pieces.next().unwrap_or_default());
    for (index, piece) in pieces.enumerate() {
        output.push_str(arguments.get(index).copied().unwrap_or("{}"));
        output.push_str(piece);
    }
    output
}

#[derive(Serialize)]
pub struct Coverage {
    pub locale: &'static str,
    pub name: &'static str,
    pub catalog_keys: usize,
    pub translated_keys: usize,
    pub native_pack_pairs: usize,
    pub native_matched_keys: usize,
}

pub fn coverage() -> Vec<Coverage> {
    let data = catalog();
    let keys = &data.locales[Locale::Japanese.key()];
    Locale::ALL
        .into_iter()
        .map(|locale| {
            let chinese = locale == Locale::SimplifiedChinese;
            Coverage {
                locale: locale.key(),
                name: locale.name(),
                catalog_keys: keys.len(),
                translated_keys: if locale == Locale::English {
                    keys.len()
                } else {
                    data.locales.get(locale.key()).map_or(0, BTreeMap::len)
                },
                native_pack_pairs: if chinese { native().pairs } else { 0 },
                native_matched_keys: if chinese {
                    keys.keys()
                        .filter(|key| native().translated.contains_key(*key))
                        .count()
                } else {
                    0
                },
            }
        })
        .collect()
}
