//! Saturating retained-heap charges, excluding each owner's inline storage.
//! Capacities include allocation slack; container charges are conservative,
//! allocator-independent estimates rather than process RSS measurements.
use std::{
    collections::{BTreeMap, HashMap},
    hash::BuildHasher,
    mem::size_of,
};

const ALLOCATION_SLACK: usize = 64;

pub(crate) fn sum(values: impl IntoIterator<Item = usize>) -> usize {
    values.into_iter().fold(0, usize::saturating_add)
}

pub(crate) fn allocation(bytes: usize) -> usize {
    if bytes == 0 {
        0
    } else {
        bytes.saturating_add(ALLOCATION_SLACK)
    }
}

pub(crate) fn values<T>(values: &Vec<T>) -> usize {
    allocation(values.capacity().saturating_mul(size_of::<T>()))
}

pub(crate) fn nested<T>(values: &Vec<T>, heap: impl Fn(&T) -> usize) -> usize {
    sum(std::iter::once(self::values(values)).chain(values.iter().map(heap)))
}

pub(crate) fn string(value: &String) -> usize {
    allocation(value.capacity())
}

pub(crate) fn path(value: &std::path::PathBuf) -> usize {
    allocation(value.capacity())
}

pub(crate) fn optional<T>(value: &Option<T>, heap: impl FnOnce(&T) -> usize) -> usize {
    value.as_ref().map_or(0, heap)
}

pub(crate) fn boxed<T>(value: &T, heap: impl FnOnce(&T) -> usize) -> usize {
    allocation(size_of::<T>()).saturating_add(heap(value))
}

pub(crate) fn arc<T>(heap: usize) -> usize {
    allocation(size_of::<T>().saturating_add(2 * size_of::<usize>())).saturating_add(heap)
}

pub(crate) fn btree<K: Ord, V>(
    map: &BTreeMap<K, V>,
    key_heap: impl Fn(&K) -> usize,
    value_heap: impl Fn(&V) -> usize,
) -> usize {
    // Reserve a complete maximum-fanout node per live entry, including edges.
    let node = sum([
        size_of::<(K, V)>().saturating_mul(11),
        size_of::<usize>().saturating_mul(16),
    ]);
    let storage = map.len().max(1).saturating_mul(allocation(node));
    sum(std::iter::once(storage).chain(map.iter().map(|(k, v)| sum([key_heap(k), value_heap(v)]))))
}

pub(crate) fn hash<K, V, S: BuildHasher>(
    map: &HashMap<K, V, S>,
    key_heap: impl Fn(&K) -> usize,
    value_heap: impl Fn(&V) -> usize,
) -> usize {
    let storage = if map.capacity() == 0 {
        0
    } else {
        let buckets = map.capacity().saturating_mul(2).max(16);
        allocation(sum([
            buckets.saturating_mul(size_of::<(K, V)>()),
            buckets,
            64,
        ]))
    };
    sum(std::iter::once(storage).chain(map.iter().map(|(k, v)| sum([key_heap(k), value_heap(v)]))))
}
