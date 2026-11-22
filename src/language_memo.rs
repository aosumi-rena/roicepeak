use crate::{
    Result,
    graph::{Budget, Graph},
    invalid,
    tensor::{Tensor, checked_elements},
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

const MAX_WORDS: usize = 512;
const MAX_IDS: usize = 64;
const MAX_CHANNELS: usize = 512;

#[derive(Debug, Default)]
struct Entries {
    rows: HashMap<Arc<[usize]>, Arc<[f32]>>,
    order: VecDeque<Arc<[usize]>>,
}

#[derive(Debug, Default)]
pub(super) struct Words(Mutex<Entries>);

enum Row {
    Cached(Arc<[f32]>),
    Computed(usize),
}

impl Row {
    fn values<'a>(&'a self, computed: Option<&'a Tensor>) -> &'a [f32] {
        match self {
            Self::Cached(values) => values,
            Self::Computed(index) => computed.unwrap().row(*index),
        }
    }
}

fn independent_rows(graph: &Graph) -> bool {
    match graph {
        Graph::Sequential(nodes) => nodes.iter().all(independent_rows),
        Graph::Affine(_) | Graph::Activation(_) | Graph::GeneratedActivation(_) => true,
        _ => false,
    }
}

impl Words {
    pub(super) fn for_graphs(graphs: [&Graph; 3]) -> Option<Self> {
        graphs.into_iter().all(independent_rows).then(Self::default)
    }

    pub(super) fn estimated_bytes(&self) -> usize {
        MAX_WORDS * (MAX_IDS * std::mem::size_of::<usize>() + MAX_CHANNELS * 4 + 256)
    }

    pub(super) fn encode(
        &self,
        ids: Vec<Vec<usize>>,
        budget: &mut Budget,
        encode: impl FnOnce(&[Vec<usize>], &mut Budget) -> Result<Tensor>,
    ) -> Result<Tensor> {
        let mut missing = Vec::new();
        let mut indices = HashMap::new();
        let rows: Vec<_> = {
            let cache = self.0.lock().ok();
            ids.into_iter()
                .map(|ids| {
                    if let Some(row) = cache
                        .as_ref()
                        .and_then(|cache| cache.rows.get(ids.as_slice()))
                    {
                        return Row::Cached(Arc::clone(row));
                    }
                    let index = *indices.entry(ids.clone()).or_insert_with(|| {
                        let index = missing.len();
                        missing.push(ids);
                        index
                    });
                    Row::Computed(index)
                })
                .collect()
        };
        let computed = if missing.is_empty() {
            None
        } else {
            let output = encode(&missing, budget)?;
            self.remember(missing, &output);
            if output.rows() == rows.len() {
                return Ok(output);
            }
            Some(output)
        };

        let width = rows[0].values(computed.as_ref()).len();
        budget.spend(0, checked_elements(rows.len(), width)?)?;
        let mut output = Tensor::zeros(rows.len(), width)?;
        for (index, row) in rows.iter().enumerate() {
            let source = row.values(computed.as_ref());
            if source.len() != width {
                return Err(invalid("word embedding widths differ"));
            }
            output.row_mut(index).copy_from_slice(source);
        }
        Ok(output)
    }

    fn remember(&self, missing: Vec<Vec<usize>>, output: &Tensor) {
        if output.cols() > MAX_CHANNELS {
            return;
        }
        let Ok(mut cache) = self.0.lock() else {
            return;
        };
        for (index, key) in missing.into_iter().enumerate() {
            if key.len() > MAX_IDS || cache.rows.contains_key(key.as_slice()) {
                continue;
            }
            if cache.rows.len() == MAX_WORDS
                && let Some(oldest) = cache.order.pop_front()
            {
                cache.rows.remove(&oldest);
            }
            let key: Arc<[usize]> = key.into();
            cache.order.push_back(Arc::clone(&key));
            cache.rows.insert(key, output.row(index).into());
        }
    }
}
