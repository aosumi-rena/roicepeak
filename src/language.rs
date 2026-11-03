//! Character CNN and word transformer for explicitly supplied language models.
//! Character caches are accepted only as explicit, identity-matched model inputs.
use std::collections::HashMap;

use crate::{
    Result,
    dnni::Node,
    graph::{Budget, Graph},
    invalid,
    tensor::{Tensor, u32_at},
};

#[derive(Debug)]
pub struct CharacterEncoder {
    pub cache_id: [u8; 8],
    alphabet: HashMap<char, usize>,
    table: Tensor,
    convolutions: Vec<Graph>,
    transform: Graph,
    gate: Graph,
    projection: Graph,
}
impl CharacterEncoder {
    pub fn estimated_bytes(&self) -> usize {
        use crate::memory as m;
        m::sum([
            m::hash(&self.alphabet, |_| 0, |_| 0),
            self.table.estimated_bytes(),
            m::nested(&self.convolutions, Graph::estimated_bytes),
            self.transform.estimated_bytes(),
            self.gate.estimated_bytes(),
            self.projection.estimated_bytes(),
        ])
    }

    pub fn compile(n: &Node<'_>) -> Result<Self> {
        if n.tag != "_lmce0" || n.children.len() != 4 || n.payload.len() < 16 {
            return Err(invalid("character encoder structure"));
        }
        let count = u32_at(n.payload, 8)? as usize;
        if count > 1_000_000 || n.payload.len() != 16 + count * 4 {
            return Err(invalid("character alphabet length"));
        }
        let mut alphabet = HashMap::new();
        for i in 0..count {
            let ch = char::from_u32(u32_at(n.payload, 16 + i * 4)?)
                .ok_or_else(|| invalid("invalid alphabet Unicode scalar"))?;
            alphabet.insert(ch, i + 4);
        }
        let [embedding, convs, highway, projection] = n.children.as_slice() else {
            unreachable!()
        };
        if embedding.tag != "modl4"
            || embedding.children.len() != 1
            || convs.children.is_empty()
            || highway.children.len() != 2
        {
            return Err(invalid("character encoder subgraphs"));
        }
        let table = embedding.children[0].tensor()?;
        if table.cols() != count + 4 {
            return Err(invalid("character embedding alphabet dimension"));
        }
        Ok(Self {
            cache_id: n.payload[..8].try_into().unwrap(),
            alphabet,
            table,
            convolutions: convs
                .children
                .iter()
                .map(Graph::compile)
                .collect::<Result<_>>()?,
            transform: Graph::compile(&highway.children[0])?,
            gate: Graph::compile(&highway.children[1])?,
            projection: Graph::compile(projection)?,
        })
    }
    pub fn ids(&self, word: &str) -> [usize; 50] {
        let mut ids = [0; 50];
        ids[0] = 1;
        let mut end = 1;
        for ch in word.chars().take(48) {
            ids[end] = self.alphabet.get(&ch).copied().unwrap_or(3);
            end += 1;
        }
        ids[end] = 2;
        ids
    }
    pub fn encode(&self, words: &[String], budget: &mut Budget) -> Result<Tensor> {
        let ids: Vec<_> = words.iter().map(|w| self.ids(w)).collect();
        self.encode_ids(&ids, budget)
    }
    /// Special boundary tokens can supply BOW/EOW directly, followed by padding.
    pub fn encode_ids(&self, words: &[[usize; 50]], budget: &mut Budget) -> Result<Tensor> {
        if words.is_empty() || words.len() > 4096 {
            return Err(invalid("word count outside 1..4096"));
        }
        let mut maxima = Vec::new();
        let mut width = None;
        for ids in words {
            let mut embedded = Tensor::zeros(50, self.table.rows())?;
            budget.spend(0, embedded.values().len())?;
            for (t, id) in ids.iter().copied().enumerate() {
                if id >= self.table.cols() {
                    return Err(invalid("character ID outside table"));
                }
                for c in 0..self.table.rows() {
                    embedded.row_mut(t)[c] = self.table.row(c)[id];
                }
            }
            let start = maxima.len();
            for conv in &self.convolutions {
                let y = conv.run_with(&embedded, budget)?;
                if y.rows() == 0 {
                    return Err(invalid("empty character convolution"));
                }
                for c in 0..y.cols() {
                    maxima.push(
                        (0..y.rows())
                            .map(|t| y.row(t)[c])
                            .fold(f32::NEG_INFINITY, f32::max)
                            .tanh(),
                    );
                }
            }
            let cols = maxima.len() - start;
            if width.replace(cols).is_some_and(|old| old != cols) {
                return Err(invalid("word embedding widths differ"));
            }
        }
        let h = Tensor::new(words.len(), width.unwrap(), maxima)?;
        let g = self.gate.run_with(&h, budget)?;
        let transformed = self.transform.run_with(&h, budget)?;
        if g.rows() != h.rows()
            || g.cols() != h.cols()
            || transformed.rows() != h.rows()
            || transformed.cols() != h.cols()
        {
            return Err(invalid("highway dimensions"));
        }
        budget.spend(h.values().len().saturating_mul(3), h.values().len())?;
        let mixed = Tensor::new(
            h.rows(),
            h.cols(),
            h.values()
                .iter()
                .zip(g.values())
                .zip(transformed.values())
                .map(|((&h, &g), &t)| g * t + (1.0 - g) * h)
                .collect(),
        )?;
        self.projection.run_with(&mixed, budget)
    }
}

#[derive(Debug)]
pub struct CharacterCache {
    identity: [u8; 8],
    words: HashMap<String, usize>,
    embeddings: Tensor,
}
impl CharacterCache {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            crate::memory::hash(&self.words, crate::memory::string, |_| 0),
            self.embeddings.estimated_bytes(),
        ])
    }

    /// Plain CharEmbeds entries: descriptor, NUL-terminated UTF-8 keys, float32 rows.
    /// Container discovery and payload extraction belong to the caller.
    pub fn parse(descriptor: &[u8], keys: &[u8], data: &[u8]) -> Result<Self> {
        if descriptor.len() < 8
            || descriptor.len() > 65536
            || keys.is_empty()
            || keys.last() != Some(&0)
            || keys.len() > 16 * 1024 * 1024
            || data.len() > crate::tensor::MAX_ELEMENTS * 4
        {
            return Err(invalid("character cache descriptor or key terminator"));
        }
        let rows: Vec<_> = keys[..keys.len() - 1].split(|b| *b == 0).collect();
        let row_bytes = rows
            .len()
            .checked_mul(4)
            .ok_or_else(|| invalid("character cache row overflow"))?;
        if data.is_empty() || data.len() % row_bytes != 0 {
            return Err(invalid("character cache data dimensions"));
        }
        let embeddings = Tensor::new(
            rows.len(),
            data.len() / row_bytes,
            crate::tensor::floats(data),
        )?;
        let mut words = HashMap::new();
        for (row, key) in rows.into_iter().enumerate() {
            let word = std::str::from_utf8(key)
                .map_err(|_| invalid("character cache key must be UTF-8"))?;
            // Repeated keys select the last stored row, as in the reference format.
            words.insert(word.to_owned(), row);
        }
        Ok(Self {
            identity: descriptor[..8].try_into().unwrap(),
            words,
            embeddings,
        })
    }
}

#[derive(Debug)]
pub struct LanguageModel {
    pub encoder: CharacterEncoder,
    layers: Vec<Graph>,
    cache: Option<CharacterCache>,
}
impl LanguageModel {
    pub fn estimated_bytes(&self) -> usize {
        crate::memory::sum([
            self.encoder.estimated_bytes(),
            crate::memory::nested(&self.layers, Graph::estimated_bytes),
            crate::memory::optional(&self.cache, CharacterCache::estimated_bytes),
        ])
    }

    pub fn compile(root: &Node<'_>) -> Result<Self> {
        if root.tag != "_lmbrt1" || root.children.len() != 2 {
            return Err(invalid("language model root"));
        }
        let bert = &root.children[1];
        if bert.tag != "_lmbrt0" || bert.children.len() != 1 {
            return Err(invalid("language transformer structure"));
        }
        Ok(Self {
            encoder: CharacterEncoder::compile(&root.children[0])?,
            layers: bert.children[0]
                .children
                .iter()
                .map(Graph::compile)
                .collect::<Result<_>>()?,
            cache: None,
        })
    }
    pub fn set_character_cache(&mut self, cache: CharacterCache) -> Result<()> {
        if cache.identity != self.encoder.cache_id {
            return Err(invalid("character cache does not belong to language model"));
        }
        self.cache = Some(cache);
        Ok(())
    }
    /// Exact cached words use their supplied rows; other words use the character CNN.
    pub fn embed(&self, words: &[String], budget: &mut Budget) -> Result<Tensor> {
        if words.is_empty() || words.len() > 4096 {
            return Err(invalid("word count outside 1..4096"));
        }
        let mut missing = Vec::new();
        let mut indices = HashMap::new();
        for word in words {
            if self
                .cache
                .as_ref()
                .is_some_and(|c| c.words.contains_key(word))
                || indices.contains_key(word)
            {
                continue;
            }
            indices.insert(word, missing.len());
            missing.push(word.clone());
        }
        let computed = if missing.is_empty() {
            None
        } else {
            Some(self.encoder.encode(&missing, budget)?)
        };
        let width = self
            .cache
            .as_ref()
            .map(|c| c.embeddings.cols())
            .or_else(|| computed.as_ref().map(Tensor::cols))
            .ok_or_else(|| invalid("no character embeddings"))?;
        if computed.as_ref().is_some_and(|t| t.cols() != width) {
            return Err(invalid("character cache and encoder widths differ"));
        }
        let mut out = Tensor::zeros(words.len(), width)?;
        budget.spend(0, out.values().len())?;
        for (row, word) in words.iter().enumerate() {
            let values = match &self.cache {
                Some(cache) if cache.words.contains_key(word) => {
                    cache.embeddings.row(cache.words[word])
                }
                _ => computed.as_ref().unwrap().row(indices[word]),
            };
            out.row_mut(row).copy_from_slice(values);
        }
        Ok(out)
    }
    /// Reference model graph only. Native positional-encoding behavior is unqualified.
    pub fn contextualize(&self, embeddings: &Tensor, budget: &mut Budget) -> Result<Tensor> {
        let mut x = embeddings.clone();
        for layer in &self.layers {
            x = layer.run_with(&x, budget)?;
        }
        Ok(x)
    }
    pub fn run(&self, words: &[String], budget: &mut Budget) -> Result<Tensor> {
        self.contextualize(&self.embed(words, budget)?, budget)
    }
    pub fn run_tokens(
        &self,
        tokens: &[crate::phonemes::Token],
        budget: &mut Budget,
    ) -> Result<Tensor> {
        if tokens.is_empty() || tokens.len() > 4096 {
            return Err(invalid("token count outside 1..4096"));
        }
        let words: Vec<_> = tokens
            .iter()
            .filter(|t| t.pos != 0x2000 && t.pos != 0x2001)
            .map(|t| t.text.clone())
            .collect();
        let words = if words.is_empty() {
            None
        } else {
            Some(self.embed(&words, budget)?)
        };
        let mut start = [0; 50];
        let mut end = [0; 50];
        start[0] = 1;
        end[0] = 2;
        let specials = self.encoder.encode_ids(&[start, end], budget)?;
        if words.as_ref().is_some_and(|t| t.cols() != specials.cols()) {
            return Err(invalid("character cache and boundary widths differ"));
        }
        let mut embeddings = Tensor::zeros(tokens.len(), specials.cols())?;
        budget.spend(0, embeddings.values().len())?;
        let mut word = 0;
        for (row, token) in tokens.iter().enumerate() {
            let values = match token.pos {
                0x2000 => specials.row(0),
                0x2001 => specials.row(1),
                _ => {
                    let values = words.as_ref().unwrap().row(word);
                    word += 1;
                    values
                }
            };
            embeddings.row_mut(row).copy_from_slice(values);
        }
        self.contextualize(&embeddings, budget)
    }
}
