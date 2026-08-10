use crate::cache::CacheKey;
use dashmap::mapref::entry::Entry;
use hickory_proto::op::Message;
use rustc_hash::FxBuildHasher;
use std::sync::Arc;
use tokio::sync::{Notify, OnceCell};

#[derive(Clone)]
pub struct CoalescingMap {
    pending: Arc<dashmap::DashMap<CacheKey, Arc<CoalesceEntry>, FxBuildHasher>>,
}

pub struct CoalesceEntry {
    pub cell: OnceCell<Result<Message, String>>,
    pub notify: Notify,
}

impl Default for CoalesceEntry {
    fn default() -> Self {
        Self::new()
    }
}

impl CoalesceEntry {
    pub fn new() -> Self {
        Self {
            cell: OnceCell::new(),
            notify: Notify::new(),
        }
    }
}

impl CoalescingMap {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(dashmap::DashMap::with_hasher(FxBuildHasher)),
        }
    }

    pub fn entry(&self, key: CacheKey) -> Entry<'_, CacheKey, Arc<CoalesceEntry>, FxBuildHasher> {
        self.pending.entry(key)
    }

    pub fn remove(&self, key: &CacheKey) {
        self.pending.remove(key);
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.len() == 0
    }
}

impl Default for CoalescingMap {
    fn default() -> Self {
        Self::new()
    }
}
