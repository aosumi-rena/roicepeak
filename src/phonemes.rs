//! Model-owned categorical features for explicit token/syllable/phone trees.
use crate::{
    Result,
    dnni::Node,
    invalid,
    prosody::{CODES, FeatureFormat},
    tensor::{Tensor, u32_at},
};
use std::collections::BTreeMap;

pub const NONE: u32 = 0x1000;
pub const LOW: u32 = 0x2000;
pub const HIGH: u32 = 0x2001;
pub struct Phone {
    pub symbol: String,
    pub nucleus: bool,
}
pub struct Syllable {
    pub reading: String,
    pub code: u32,
    pub phones: Vec<Phone>,
}
pub struct Token {
    pub text: String,
    pub language: u32,
    pub pos: u32,
    pub syllables: Vec<Syllable>,
}
pub struct Encoded {
    /// phone ID, language ID, then feature channels.
    pub linguistic: Tensor,
    /// token, syllable-in-token, phone-in-syllable, global syllable, global phone.
    pub references: Vec<[usize; 5]>,
    pub nucleus: Vec<bool>,
    pub codes: Vec<u32>,
    pub symbols: Vec<String>,
}
impl Encoded {
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        m::sum([
            self.linguistic.estimated_bytes(),
            m::values(&self.references),
            m::values(&self.nucleus),
            m::values(&self.codes),
            m::nested(&self.symbols, m::string),
        ])
    }
}
pub(crate) struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    pub(crate) fn number(&mut self) -> Result<usize> {
        let n = u32_at(self.bytes, self.at)? as usize;
        self.at += 4;
        Ok(n)
    }
    pub(crate) fn string(&mut self) -> Result<String> {
        let n = self.number()?;
        let end = self
            .at
            .checked_add(n)
            .ok_or_else(|| invalid("string length overflow"))?;
        let s = std::str::from_utf8(
            self.bytes
                .get(self.at..end)
                .ok_or_else(|| invalid("truncated string"))?,
        )
        .map_err(|_| invalid("non-UTF8 model string"))?
        .to_owned();
        self.at = end;
        Ok(s)
    }
    pub(crate) fn strings(&mut self) -> Result<Vec<String>> {
        let n = self.number()?;
        if n > 100_000 {
            return Err(invalid("string table too large"));
        }
        (0..n).map(|_| self.string()).collect()
    }
    pub(crate) fn finish(&self) -> Result<()> {
        if self.at != self.bytes.len() {
            return Err(invalid("trailing string table data"));
        }
        Ok(())
    }
}
struct PhoneSet {
    name: String,
    symbols: Vec<String>,
    mapped: Vec<String>,
}
impl PhoneSet {
    fn parse(node: &Node<'_>) -> Result<Self> {
        let mut c = Cursor::new(node.payload);
        let name = c.string()?;
        let symbols = c.strings()?;
        let classes = c.strings()?;
        let mapped = c.strings()?;
        let _class_names = c.strings()?;
        c.finish()?;
        if symbols.len() != mapped.len() || symbols.len() != classes.len() {
            return Err(invalid("phone set dimensions"));
        }
        Ok(Self {
            name,
            symbols,
            mapped,
        })
    }
}
pub struct PhonemeEncoder {
    legacy: bool,
    categorical: Vec<usize>,
    boolean: Vec<usize>,
    tables: Vec<BTreeMap<String, usize>>,
    offsets: Vec<usize>,
    phone_sets: Vec<PhoneSet>,
    unified: BTreeMap<String, usize>,
    pub width: usize,
    pub format: FeatureFormat,
}
impl PhonemeEncoder {
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        let strings = |values: &Vec<String>| m::nested(values, m::string);
        let table = |values: &BTreeMap<String, usize>| m::btree(values, m::string, |_| 0);
        m::sum([
            m::values(&self.categorical),
            m::values(&self.boolean),
            m::values(&self.offsets),
            m::nested(&self.tables, table),
            m::nested(&self.phone_sets, |set| {
                m::sum([
                    m::string(&set.name),
                    strings(&set.symbols),
                    strings(&set.mapped),
                ])
            }),
            table(&self.unified),
        ])
    }

    pub fn compile(root: &Node<'_>) -> Result<Self> {
        let legacy = root.tag == "_phenc0";
        if (!legacy && root.tag != "_phenc1")
            || root.payload.len() != 4
            || u32_at(root.payload, 0)? != u32::from(!legacy)
            || root.children.len() != if legacy { 1 } else { 2 }
        {
            return Err(invalid("phoneme encoder structure"));
        }
        let table = &root.children[usize::from(!legacy)];
        if table.tag != "_taffv0" {
            return Err(invalid("categorical feature table"));
        }
        let count = u32_at(table.payload, 0)? as usize;
        let cats = u32_at(table.payload, 4)? as usize;
        let bools = u32_at(table.payload, 8)? as usize;
        let nums = u32_at(table.payload, 12)? as usize;
        if count != cats
            || nums != 0
            || cats > 1024
            || bools > 1024
            || table.children.len() != cats + 1
            || table.payload.len() != 16 + (cats + bools) * 4
        {
            return Err(invalid("categorical feature layout"));
        }
        let categorical = (0..cats)
            .map(|i| u32_at(table.payload, 16 + i * 4).map(|n| n as usize))
            .collect::<Result<Vec<_>>>()?;
        let boolean = (0..bools)
            .map(|i| u32_at(table.payload, 16 + (cats + i) * 4).map(|n| n as usize))
            .collect::<Result<Vec<_>>>()?;
        let mut tables = Vec::new();
        let mut offsets = vec![0];
        for node in &table.children[1..] {
            let mut c = Cursor::new(node.payload);
            let strings = c.strings()?;
            c.finish()?;
            let mut map = BTreeMap::new();
            for (i, s) in strings.into_iter().enumerate() {
                if map.insert(s, i + 1).is_some() {
                    return Err(invalid("duplicate category"));
                }
            }
            offsets.push(offsets.last().unwrap() + map.len() + 1);
            tables.push(map);
        }
        let width = offsets.last().unwrap() + bools;
        if width > 65536 {
            return Err(invalid("feature width too large"));
        }
        let group = categorical
            .iter()
            .position(|c| *c == if legacy { 3 } else { 2 })
            .ok_or_else(|| invalid("no accent category"))?;
        let names = [
            "_", "L", "H", "man1", "man2", "man3", "man4", "man5", "en0", "en1", "en2",
        ];
        let slots = std::array::from_fn(|i| {
            offsets[group] + tables[group].get(names[i]).copied().unwrap_or(0)
        });
        let mut phone_sets = Vec::new();
        let mut unified = BTreeMap::new();
        let mut japanese = 0;
        if !legacy {
            let f = &root.children[0];
            if f.children.len() != 2 {
                return Err(invalid("phone formatter structure"));
            }
            for n in &f.children[0].children {
                phone_sets.push(PhoneSet::parse(n)?);
            }
            let set = PhoneSet::parse(&f.children[1])?;
            unified = set
                .symbols
                .into_iter()
                .enumerate()
                .map(|(i, s)| (s, i))
                .collect();
            japanese = phone_sets
                .iter()
                .position(|s| s.name.contains("japanese"))
                .ok_or_else(|| invalid("no Japanese phone set"))? as u32;
            if !unified.contains_key("sil") {
                return Err(invalid("missing silence phone"));
            }
        }
        Ok(Self {
            legacy,
            categorical,
            boolean,
            tables,
            offsets,
            phone_sets,
            unified,
            width,
            format: FeatureFormat { japanese, slots },
        })
    }
    fn language(&self, language: u32) -> Result<usize> {
        let name = match language {
            1 => "japanese",
            2 => "mandarin",
            3 => "english",
            _ => return Err(invalid("unsupported token language")),
        };
        self.phone_sets
            .iter()
            .position(|s| s.name.contains(name))
            .ok_or_else(|| invalid("language missing from phone formatter"))
    }
    pub fn encode(&self, tokens: &[Token]) -> Result<Encoded> {
        if tokens.len() > 4096 {
            return Err(invalid("too many tokens"));
        }
        let mut values = Vec::new();
        let mut references = Vec::new();
        let mut nucleus = Vec::new();
        let mut codes = Vec::new();
        let mut symbols = Vec::new();
        let mut slot = 0usize;
        let mut global = 0usize;
        for (t, token) in tokens.iter().enumerate() {
            if token.syllables.len() > 100_000 {
                return Err(invalid("too many syllables"));
            }
            if !self.legacy && !(1..=3).contains(&token.language) {
                slot += token.syllables.len();
                global += token
                    .syllables
                    .iter()
                    .map(|s| s.phones.len())
                    .sum::<usize>();
                continue;
            }
            let lang = if self.legacy {
                0
            } else {
                self.language(token.language)?
            };
            let pos = if token.language != 1 {
                ""
            } else {
                pos_name(token.pos)
            };
            for (s, syllable) in token.syllables.iter().enumerate() {
                for (p, phone) in syllable.phones.iter().enumerate() {
                    if references.len() >= 100_000 || phone.symbol.len() > 4096 {
                        return Err(invalid("phoneme input budget"));
                    }
                    let code = if phone.nucleus { syllable.code } else { NONE };
                    let code_name = code_name(code)?;
                    let first = if s == 0 && p == 0 { "1" } else { "" };
                    let fields = if self.legacy {
                        vec![phone.symbol.as_str(), pos, first, code_name]
                    } else {
                        vec![pos, first, code_name]
                    };
                    let phone_id = if self.legacy {
                        let group = self
                            .categorical
                            .iter()
                            .position(|c| *c == 0)
                            .ok_or_else(|| invalid("no phone category"))?;
                        self.tables[group].get(&phone.symbol).copied().unwrap_or(0)
                    } else {
                        let set = &self.phone_sets[lang];
                        let mapped = set
                            .symbols
                            .iter()
                            .position(|s| s == &phone.symbol)
                            .map(|i| set.mapped[i].as_str())
                            .unwrap_or(&phone.symbol);
                        self.unified
                            .get(mapped)
                            .copied()
                            .unwrap_or(self.unified["sil"])
                    };
                    let mut row = vec![0.0; self.width + 2];
                    row[0] = phone_id as f32;
                    row[1] = lang as f32;
                    for (i, column) in self.categorical.iter().enumerate() {
                        let field = fields
                            .get(*column)
                            .ok_or_else(|| invalid("categorical column outside input"))?;
                        row[2
                            + self.offsets[i]
                            + self.tables[i].get(*field).copied().unwrap_or(0)] = 1.0;
                    }
                    for (i, column) in self.boolean.iter().enumerate() {
                        let field = fields
                            .get(*column)
                            .ok_or_else(|| invalid("boolean column outside input"))?;
                        row[2 + self.offsets.last().unwrap() + i] =
                            f32::from(["1", "true", "True"].contains(field));
                    }
                    values.extend(row);
                    references.push([t, s, p, slot, global]);
                    nucleus.push(phone.nucleus);
                    codes.push(code);
                    symbols.push(phone.symbol.clone());
                    global += 1;
                }
                slot += 1;
            }
        }
        Ok(Encoded {
            linguistic: Tensor::new(references.len(), self.width + 2, values)?,
            references,
            nucleus,
            codes,
            symbols,
        })
    }
}
pub fn code_name(code: u32) -> Result<&'static str> {
    let names = [
        "_", "L", "H", "man1", "man2", "man3", "man4", "man5", "en0", "en1", "en2",
    ];
    CODES
        .iter()
        .position(|c| *c == code)
        .map(|i| names[i])
        .ok_or_else(|| invalid("unknown syllable code"))
}
pub fn pos_name(pos: u32) -> &'static str {
    const POS: [&str; 16] = [
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
    pos.checked_sub(0x1000)
        .and_then(|i| POS.get(i as usize))
        .copied()
        .unwrap_or("補助記号")
}

/// Nine tab-separated columns: token index, text, language, POS, syllable index,
/// reading, accent code, phone symbol, nucleus (0/1). Numeric fields are decimal.
/// Repeated rows must agree on their token and syllable metadata.
pub fn parse_tsv(text: &str) -> Result<Vec<Token>> {
    if text.len() > 16 * 1024 * 1024 {
        return Err(invalid("token TSV exceeds byte budget"));
    }
    let mut tokens: Vec<Token> = Vec::new();
    for line in text.lines().filter(|l| !l.is_empty()) {
        let f: Vec<_> = line.split('\t').collect();
        if f.len() != 9 {
            return Err(invalid("token TSV needs nine columns"));
        }
        let number = |i: usize| {
            f[i].parse::<u32>()
                .map_err(|_| invalid("token TSV requires decimal unsigned integers"))
        };
        let t = number(0)? as usize;
        let language = number(2)?;
        let pos = number(3)?;
        let s = number(4)? as usize;
        let code = number(6)?;
        code_name(code)?;
        if t > tokens.len() || t >= 4096 {
            return Err(invalid(
                "token indices must start at zero and be consecutive",
            ));
        }
        if t == tokens.len() {
            tokens.push(Token {
                text: f[1].into(),
                language,
                pos,
                syllables: Vec::new(),
            });
        }
        if t + 1 != tokens.len() {
            return Err(invalid("token rows must be contiguous"));
        }
        let token = &mut tokens[t];
        if token.text != f[1] || token.language != language || token.pos != pos {
            return Err(invalid("inconsistent repeated token metadata"));
        }
        if s > token.syllables.len() || s >= 100_000 {
            return Err(invalid("syllable indices must be consecutive"));
        }
        if s == token.syllables.len() {
            token.syllables.push(Syllable {
                reading: f[5].into(),
                code,
                phones: Vec::new(),
            });
        }
        if s + 1 != token.syllables.len() {
            return Err(invalid("syllable rows must be contiguous"));
        }
        let syllable = &mut token.syllables[s];
        if syllable.reading != f[5] || syllable.code != code {
            return Err(invalid("inconsistent repeated syllable metadata"));
        }
        let nucleus = match f[8] {
            "0" => false,
            "1" => true,
            _ => return Err(invalid("nucleus must be zero or one")),
        };
        if syllable.phones.len() >= 1024 || f[7].len() > 4096 {
            return Err(invalid("syllable phone budget"));
        }
        syllable.phones.push(Phone {
            symbol: f[7].into(),
            nucleus,
        });
    }
    Ok(tokens)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_tsv_rejects_noncontiguous_and_inconsistent_groups() {
        let good = "0\tか\t1\t4096\t0\tカ\t8193\tk\t0\n0\tか\t1\t4096\t0\tカ\t8193\ta\t1\n";
        let tokens = parse_tsv(good).unwrap();
        assert_eq!(tokens[0].syllables[0].phones.len(), 2);
        assert!(parse_tsv(&good.replace("\tカ\t8193\ta", "\tガ\t8193\ta")).is_err());
        assert!(parse_tsv(&good.replace("0\tか", "1\tか")).is_err());
    }
}
