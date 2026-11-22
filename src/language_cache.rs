use crate::{Result, dnni::Node};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};

const MAX_SHARED_MODELS: usize = 16;
pub(super) type Shared<T> = Mutex<BTreeMap<[u8; 32], Weak<T>>>;

fn hash_node(node: &Node<'_>, payload: &[u8], digest: &mut Sha256) {
    digest.update([u8::from(node.composite)]);
    digest.update((node.tag.len() as u64).to_le_bytes());
    digest.update(node.tag.as_bytes());
    digest.update((payload.len() as u64).to_le_bytes());
    digest.update(payload);
    digest.update((node.children.len() as u64).to_le_bytes());
    for child in &node.children {
        hash_node(child, child.payload, digest);
    }
}

pub(super) fn compile<T>(
    cache: &Shared<T>,
    root: &Node<'_>,
    payload: &[u8],
    build: impl FnOnce() -> Result<T>,
) -> Result<Arc<T>> {
    let mut digest = Sha256::new();
    hash_node(root, payload, &mut digest);
    let key = digest.finalize().into();
    if let Some(model) = cache
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).and_then(Weak::upgrade))
    {
        return Ok(model);
    }

    let compiled = Arc::new(build()?);
    if let Ok(mut cache) = cache.lock() {
        if let Some(model) = cache.get(&key).and_then(Weak::upgrade) {
            return Ok(model);
        }
        cache.retain(|_, model| model.strong_count() != 0);
        if cache.len() < MAX_SHARED_MODELS {
            cache.insert(key, Arc::downgrade(&compiled));
        }
    }
    Ok(compiled)
}
