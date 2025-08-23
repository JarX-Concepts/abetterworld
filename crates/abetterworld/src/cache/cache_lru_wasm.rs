use bytes::Bytes;
use std::collections::HashMap;
use std::sync::RwLock;

use crate::cache::types::TilesetMemoryCache;

pub struct WasmCache {
    inner: RwLock<HashMap<u64, (String, Bytes)>>,
}

impl WasmCache {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }
}

impl TilesetMemoryCache for WasmCache {
    fn get(&self, key: u64) -> Option<(String, Bytes)> {
        self.inner.read().ok()?.get(&key).cloned()
    }

    fn insert(&self, key: u64, value: (String, Bytes)) {
        if let Ok(mut map) = self.inner.write() {
            map.insert(key, value);
        }
    }

    fn invalidate_all(&self) {
        if let Ok(mut map) = self.inner.write() {
            map.clear();
        }
    }
}
