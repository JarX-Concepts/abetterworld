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
    base_dir: std::path::PathBuf,
}

#[cfg(target_os = "android")]
fn default_cache_dir() -> std::path::PathBuf {
    // Requires the ndk-glue crate and that your Android entrypoint uses it.
    // internal_data_path() is private app storage: /data/data/<pkg>/files
    // Prefer `cache_path()` if you want the OS-manageable cache dir.
    let act = ndk_glue::native_activity();
    act.cache_path().to_path_buf().join("tilesets")
}

#[cfg(target_os = "ios")]
fn default_cache_dir() -> std::path::PathBuf {
    // `directories` or `dirs-next` will resolve to <App Sandbox>/Library/Caches
    directories::BaseDirs::new()
        .and_then(|b| Some(b.cache_dir().to_path_buf()))
        .unwrap_or(std::env::temp_dir())
        .join("tilesets")
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn default_cache_dir() -> std::path::PathBuf {
    std::path::PathBuf::from("./tilesets")
}

impl TilesetCache {
    pub fn new(cache_dir: &str) -> Self {
        let base_dir = cache_dir.clone().into();
        let _ = fs::create_dir_all(&base_dir);
        Self {
            map: Arc::new(NativeCache::new(LRU_CACHE_CAPACITY)),
            file_lock: RwLock::new(()),
            base_dir,
        }
    }

    pub fn disk_path_for(&self, key: &str) -> std::path::PathBuf {
        let encoded = hash_uri(key);
        self.base_dir.join(format!("{encoded}.json"))
    }

    pub async fn get(&self, key: &str) -> Result<Option<(String, Bytes)>, AbwError> {
        let id = hash_uri(key);
        if let Some((ct, data)) = self.map.get(id) {
            return Ok(Some((ct, data)));
        }

        let filename = self.disk_path_for(key);
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

        let filename = self.disk_path_for(&key);
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
}
