use crate::cache::cache_native::TilesetMemoryCache;
use bytes::Bytes;

pub struct NativeCache {
    inner: moka::sync::Cache<u64, (String, Bytes)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeCache {
    pub fn new(cap: u64) -> Self {
        Self {
            inner: moka::sync::Cache::new(cap),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl TilesetMemoryCache for NativeCache {
    fn get(&self, key: u64) -> Option<(String, Bytes)> {
        self.inner.get(&key)
    }

    fn insert(&self, key: u64, value: (String, Bytes)) {
        self.inner.insert(key, value);
    }

    fn invalidate_all(&self) {
        self.inner.invalidate_all();
    }
}
