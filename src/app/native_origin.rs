//! Immutable native document provenance shared by editor snapshots.
use crate::app::native_project::NativeProject;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;
use std::{collections::BTreeSet, sync::Arc};

const MAX_NATIVE_BYTES: usize = 16 * 1024 * 1024;
const MAX_OWNERS: usize = 500_000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(transparent)]
pub(crate) struct NativeOwner(u64);

#[derive(Clone, Copy, Debug)]
enum Node {
    Block { block: usize },
    Sentence { block: usize, sentence: usize },
}

#[derive(Debug)]
struct Data {
    document: String,
    root: Value,
    owners: Vec<Node>,
    block_offsets: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct NativeOrigin(Arc<Data>);

impl PartialEq for NativeOrigin {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.document.as_bytes() == other.0.document.as_bytes()
    }
}

impl NativeOrigin {
    pub fn new(native: NativeProject) -> Result<Self, String> {
        let bytes = native.to_bytes()?;
        if bytes.len() > MAX_NATIVE_BYTES {
            return Err("Native origin byte limit".into());
        }
        let document = String::from_utf8(bytes).map_err(|_| "Native origin must be UTF-8")?;
        let mut owners = Vec::new();
        let mut block_offsets = Vec::with_capacity(native.blocks().len());
        for (block, value) in native.blocks().iter().enumerate() {
            block_offsets.push(owners.len());
            owners.push(Node::Block { block });
            let sentences = value["sentence-list"]
                .as_array()
                .ok_or("Native sentence list missing")?;
            if owners
                .len()
                .checked_add(sentences.len())
                .is_none_or(|n| n > MAX_OWNERS)
            {
                return Err("Native origin owner limit".into());
            }
            owners.extend((0..sentences.len()).map(|sentence| Node::Sentence { block, sentence }));
        }
        Ok(Self(Arc::new(Data {
            document,
            root: native.root().clone(),
            owners,
            block_offsets,
        })))
    }

    pub fn block_owner(&self, index: usize) -> Result<NativeOwner, String> {
        self.0
            .block_offsets
            .get(index)
            .copied()
            .map(|index| NativeOwner(index as u64 + 1))
            .ok_or("Native block owner not found".into())
    }

    pub fn sentence_owner(
        &self,
        block_index: usize,
        sentence_index: usize,
    ) -> Result<NativeOwner, String> {
        let offset = *self
            .0
            .block_offsets
            .get(block_index)
            .ok_or("Native block owner not found")?;
        let index = offset
            .checked_add(1)
            .and_then(|n| n.checked_add(sentence_index))
            .ok_or("Native sentence owner overflow")?;
        match self.0.owners.get(index) {
            Some(Node::Sentence { block, sentence })
                if *block == block_index && *sentence == sentence_index =>
            {
                Ok(NativeOwner(index as u64 + 1))
            }
            _ => Err("Native sentence owner not found".into()),
        }
    }

    fn node(&self, owner: NativeOwner) -> Result<Node, String> {
        let index = owner
            .0
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
            .ok_or("Invalid native owner")?;
        self.0
            .owners
            .get(index)
            .copied()
            .ok_or("Native owner outside origin".into())
    }

    pub fn block(&self, owner: NativeOwner) -> Result<&Value, String> {
        match self.node(owner)? {
            Node::Block { block } => Ok(&self.0.root["project"]["blocks"][block]),
            _ => Err("Native owner is not a block".into()),
        }
    }

    pub fn sentence(&self, owner: NativeOwner) -> Result<&Value, String> {
        match self.node(owner)? {
            Node::Sentence { block, sentence } => {
                Ok(&self.0.root["project"]["blocks"][block]["sentence-list"][sentence])
            }
            _ => Err("Native owner is not a sentence".into()),
        }
    }

    pub fn sentence_owner_in_block(
        &self,
        owner: NativeOwner,
        index: usize,
    ) -> Result<NativeOwner, String> {
        match self.node(owner)? {
            Node::Block { block } => self.sentence_owner(block, index),
            _ => Err("Native owner is not a block".into()),
        }
    }
    pub fn root(&self) -> &Value {
        &self.0.root
    }
    pub fn original_bytes(&self) -> &[u8] {
        self.0.document.as_bytes()
    }

    /// A moved whole sentence keeps its owner even when its current block changes.
    /// Tokens are independent of reusable editor block/sentence IDs.
    pub fn validate_links(
        &self,
        blocks: impl IntoIterator<Item = Option<NativeOwner>>,
        sentences: impl IntoIterator<Item = Option<NativeOwner>>,
    ) -> Result<(), String> {
        let mut used = BTreeSet::new();
        for owner in blocks.into_iter().flatten() {
            self.block(owner)?;
            if !used.insert(owner) {
                return Err("Duplicate native block owner".into());
            }
        }
        for owner in sentences.into_iter().flatten() {
            self.sentence(owner)?;
            if !used.insert(owner) {
                return Err("Duplicate native sentence owner".into());
            }
        }
        Ok(())
    }

    pub fn estimated_bytes(&self) -> usize {
        fn retained(value: &Value) -> usize {
            let own = std::mem::size_of::<Value>();
            own.saturating_add(match value {
                Value::String(s) => s.capacity(),
                Value::Array(a) => a
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Value>())
                    .saturating_add(a.iter().map(retained).fold(0usize, usize::saturating_add)),
                Value::Object(o) => o
                    .iter()
                    .map(|(key, item)| {
                        key.capacity()
                            .saturating_add(retained(item))
                            .saturating_add(128)
                    })
                    .fold(0usize, usize::saturating_add),
                _ => 0,
            })
        }
        std::mem::size_of::<Data>()
            .saturating_add(self.0.document.capacity())
            .saturating_add(
                self.0
                    .owners
                    .capacity()
                    .saturating_mul(std::mem::size_of::<Node>()),
            )
            .saturating_add(
                self.0
                    .block_offsets
                    .capacity()
                    .saturating_mul(std::mem::size_of::<usize>()),
            )
            .saturating_add(retained(&self.0.root))
    }
}

#[derive(Serialize)]
struct Stored<'a> {
    format: u32,
    document: &'a str,
}
impl Serialize for NativeOrigin {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Stored {
            format: 1,
            document: &self.0.document,
        }
        .serialize(serializer)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Loaded {
    format: u32,
    document: String,
}
impl<'de> Deserialize<'de> for NativeOrigin {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = Loaded::deserialize(deserializer)?;
        if stored.format != 1 || stored.document.len() > MAX_NATIVE_BYTES {
            return Err(de::Error::custom(
                "Unsupported native origin format or size",
            ));
        }
        let native = NativeProject::parse(stored.document.as_bytes()).map_err(de::Error::custom)?;
        Self::new(native).map_err(de::Error::custom)
    }
}

pub(crate) fn required_document<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<NativeOrigin>, D::Error> {
    Option::<NativeOrigin>::deserialize(deserializer)
}
pub(crate) fn required_owner<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<NativeOwner>, D::Error> {
    Option::<NativeOwner>::deserialize(deserializer)
}
