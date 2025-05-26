use bytes::Bytes;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

const TILESET_CACHE_DIR: &str = "./tilesets";

#[derive(Serialize, Deserialize)]
struct DiskCacheEntry {
    content_type: String,
    data: Vec<u8>,
}

pub struct TilesetCache {
    map: DashMap<String, (String, Vec<u8>)>,
    file_lock: Arc<Mutex<()>>,
}

impl TilesetCache {
    pub fn new() -> Self {
        fs::create_dir_all(TILESET_CACHE_DIR).ok();
        Self {
            map: DashMap::new(),
            file_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn get(&self, key: &str) -> Option<(String, Bytes)> {
        // Try memory first
        if let Some(entry) = self.map.get(key) {
            let (content_type, data) = entry.value();
            return Some((content_type.clone(), Bytes::from(data.clone())));
        }

        // Try disk next
        let filename = Self::disk_path_for(key);
        if Path::new(&filename).exists() {
            let file_lock = self.file_lock.clone();
            let bytes = {
                let _guard = file_lock.lock().unwrap();
                fs::read(&filename).ok()
            };
            if let Some(bytes) = bytes {
                if let Ok(entry) = serde_json::from_slice::<DiskCacheEntry>(&bytes) {
                    self.map.insert(
                        key.to_string(),
                        (entry.content_type.clone(), entry.data.clone()),
                    );
                    return Some((entry.content_type, Bytes::from(entry.data)));
                }
            }
        }
        None
    }

    pub fn insert(&self, key: String, content_type: String, bytes: Bytes) {
        self.map
            .insert(key.clone(), (content_type.clone(), bytes.to_vec()));

        // Save to disk synchronously
        let entry = DiskCacheEntry {
            content_type,
            data: bytes.to_vec(),
        };
        let filename = Self::disk_path_for(&key);
        let bytes = serde_json::to_vec(&entry).unwrap();
        let file_lock = self.file_lock.clone();
        let _guard = file_lock.lock().unwrap();
        let _ = fs::write(filename, bytes);
    }

    fn disk_path_for(key: &str) -> String {
        use hex;
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let hash = hasher.finalize();
        let encoded = hex::encode(hash);
        format!("{}/{}.json", TILESET_CACHE_DIR, encoded)
    }
}

// Singleton instance
pub static TILESET_CACHE: Lazy<Arc<TilesetCache>> = Lazy::new(|| Arc::new(TilesetCache::new()));
