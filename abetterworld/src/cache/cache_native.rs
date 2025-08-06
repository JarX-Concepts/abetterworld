use crate::cache::cache_lru_native::NativeCache;
use crate::helpers::{hash_uri, AbwError};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock, RwLock};
use std::{fs, path::Path};

const TILESET_CACHE_DIR: &str = "./tilesets";
const LRU_CACHE_CAPACITY: u64 = 512;

#[derive(Serialize, Deserialize)]
struct DiskCacheEntry {
    id: String,
    content_type: String,
    data: Vec<u8>,
}

pub trait TilesetMemoryCache: Send + Sync {
    fn get(&self, key: u64) -> Option<(String, Bytes)>;
    fn insert(&self, key: u64, value: (String, Bytes));
    fn invalidate_all(&self);
}

pub struct TilesetCache {
    pub map: Arc<dyn TilesetMemoryCache>,
    file_lock: RwLock<()>,
}

impl TilesetCache {
    pub fn new() -> Self {
        let map: Arc<dyn TilesetMemoryCache> = Arc::new(NativeCache::new(LRU_CACHE_CAPACITY));

        fs::create_dir_all(TILESET_CACHE_DIR).ok();

        Self {
            map,
            file_lock: RwLock::new(()),
        }
    }

    pub async fn get(&self, key: &str) -> Result<Option<(String, Bytes)>, AbwError> {
        let id = hash_uri(key);
        if let Some((ct, data)) = self.map.get(id) {
            return Ok(Some((ct, data)));
        }

        let filename = Self::disk_path_for(key);
        if Path::new(&filename).exists() {
            let _guard = self
                .file_lock
                .read()
                .map_err(|_| AbwError::Io("cache get lock poisoned".into()))?;

            let bytes = fs::read(&filename)
                .map_err(|e| AbwError::Io(format!("Failed to read cache file: {e}")))?;

            let entry: DiskCacheEntry = serde_json::from_slice(&bytes)
                .map_err(|e| AbwError::Io(format!("Failed to deserialize cache entry: {e}")))?;

            let data = Bytes::from(entry.data.clone());
            self.map
                .insert(id, (entry.content_type.clone(), data.clone()));

            return Ok(Some((entry.content_type, data)));
        }

        Ok(None)
    }

    pub async fn insert(
        &self,
        key: String,
        content_type: String,
        bytes: Bytes,
    ) -> Result<(), AbwError> {
        let id = hash_uri(&key);
        self.map.insert(id, (content_type.clone(), bytes.clone()));

        let entry = DiskCacheEntry {
            id: id.to_string(),
            content_type,
            data: bytes.to_vec(),
        };

        let filename = Self::disk_path_for(&key);
        let bytes = serde_json::to_vec(&entry).unwrap();
        let _guard = self
            .file_lock
            .write()
            .map_err(|e| AbwError::Io(format!("Failed to acquire cache insert lock: {e}")))?;
        let _ = fs::write(filename, bytes);

        Ok(())
    }

    pub fn clear(&self) -> Result<(), AbwError> {
        self.map.invalidate_all();

        let _guard = self
            .file_lock
            .write()
            .map_err(|_| AbwError::Io("cache get lock poisoned".into()))?;
        if Path::new(TILESET_CACHE_DIR).exists() {
            fs::remove_dir_all(TILESET_CACHE_DIR).ok();
        }
        fs::create_dir_all(TILESET_CACHE_DIR).ok();

        Ok(())
    }

    pub fn disk_path_for(key: &str) -> String {
        use crate::helpers::hash_uri;

        let encoded = hash_uri(key);
        format!("{}/{}.json", TILESET_CACHE_DIR, encoded)
    }
}

static TILESET_CACHE: OnceLock<Arc<TilesetCache>> = OnceLock::new();

pub fn init_tileset_cache() -> Arc<TilesetCache> {
    let cache = Arc::new(TilesetCache::new());
    if TILESET_CACHE.set(cache.clone()).is_err() {
        log::warn!("TilesetCache already initialized");
    }
    cache
}

pub fn get_tileset_cache() -> Arc<TilesetCache> {
    TILESET_CACHE
        .get()
        .expect("TilesetCache not initialized")
        .clone()
}
