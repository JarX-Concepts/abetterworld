use bytes::Bytes;
use lru::LruCache;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

#[cfg(not(target_arch = "wasm32"))]
use std::{fs, path::Path};

#[cfg(target_arch = "wasm32")]
use idb::{Database, DatabaseEvent, Error, Factory, KeyPath, ObjectStoreParams, TransactionMode};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::wasm_bindgen::JsValue;

const TILESET_CACHE_DIR: &str = "./tilesets";
const LRU_CACHE_CAPACITY: std::num::NonZeroUsize = std::num::NonZeroUsize::new(512).unwrap();

#[derive(Serialize, Deserialize)]
struct DiskCacheEntry {
    id: String,
    content_type: String,
    data: Vec<u8>,
}

#[derive(Debug)]
pub struct TilesetCache {
    pub map: Mutex<LruCache<String, (String, Bytes)>>,
    #[cfg(not(target_arch = "wasm32"))]
    file_lock: Arc<Mutex<()>>,
    #[cfg(target_arch = "wasm32")]
    db: Option<Database>,
}

impl TilesetCache {
    pub fn new() -> Self {
        let map = Mutex::new(LruCache::new(LRU_CACHE_CAPACITY));

        #[cfg(not(target_arch = "wasm32"))]
        {
            fs::create_dir_all(TILESET_CACHE_DIR).ok();
        }
        #[cfg(target_arch = "wasm32")]
        {
            let factory = Factory::new().expect("Failed to create IndexedDB factory");

            let mut open_request = factory.open("abetterworld", Some(1)).unwrap();
            // Add an upgrade handler for database
            open_request.on_upgrade_needed(|event| {
                // Get database instance from event
                let database = event.database().unwrap();

                // Prepare object store params
                let mut store_params = ObjectStoreParams::new();
                store_params.auto_increment(true);
                store_params.key_path(Some(KeyPath::new_single("id")));

                // Create object store
                if !database
                    .store_names()
                    .iter()
                    .any(|name| name == "abetterworld")
                {
                    database.create_object_store("abetterworld", store_params);
                }
            });
            let database_result = open_request.await;
            match database_result {
                Ok(db) => {
                    return Self { map, db: Some(db) };
                }
                Err(e) => {
                    log::error!("Failed to open IndexedDB: {:?}", e);
                    return Self { map, db: None };
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            return Self {
                map,
                #[cfg(not(target_arch = "wasm32"))]
                file_lock: Arc::new(Mutex::new(())),
            };
        }
    }

    #[cfg(target_arch = "wasm32")]
    async fn get_idb_data(&self, id: JsValue) -> Result<Option<DiskCacheEntry>, Error> {
        let database = match self.db.as_ref() {
            Some(db) => db,
            None => {
                return Err(Error::IndexedDbNotFound(JsValue::from_str(
                    "abetterworld db not opened",
                )))
            }
        };

        // Create a read-only transaction
        let transaction = database
            .transaction(&["abetterworld"], TransactionMode::ReadOnly)
            .unwrap();

        // Get the object store
        let store = transaction.object_store("abetterworld").unwrap();

        // Get the stored data
        let stored_employee: Option<JsValue> = store.get(id)?.await?;

        // Deserialize the stored data
        let stored_employee: Option<DiskCacheEntry> = stored_employee
            .map(|stored_employee| serde_wasm_bindgen::from_value(stored_employee).unwrap());

        // Wait for the transaction to complete (alternatively, you can also commit the transaction)
        transaction.await?;

        Ok(stored_employee)
    }

    #[cfg(target_arch = "wasm32")]
    async fn insert_idb_data(&self, entry: DiskCacheEntry) -> Result<JsValue, Error> {
        let database = match self.db.as_ref() {
            Some(db) => db,
            None => {
                return Err(Error::IndexedDbNotFound(JsValue::from_str(
                    "abetterworld db not opened",
                )))
            }
        };

        let transaction = database.transaction(&["abetterworld"], TransactionMode::ReadWrite)?;

        // Get the object store
        let store = transaction.object_store("abetterworld").unwrap();

        // Serialize the entry to JsValue using serde_wasm_bindgen
        let js_value = serde_wasm_bindgen::to_value(&entry).unwrap();

        // Add data to object store
        let id = store.add(&js_value, None).unwrap().await?;

        // Commit the transaction
        transaction.commit()?.await?;

        Ok(id)
    }

    pub fn get(&self, key: &str) -> Option<(String, Bytes)> {
        let mut map = self.map.lock()?;
        if let Some((ct, data)) = map.get(key).cloned() {
            return Some((ct, data));
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let filename = Self::disk_path_for(key);
            if Path::new(&filename).exists() {
                let bytes = {
                    let _guard = self.file_lock.lock().unwrap();
                    fs::read(&filename).ok()
                }?;
                let entry: DiskCacheEntry = serde_json::from_slice(&bytes).ok()?;
                let data = Bytes::from(entry.data.clone());
                map.put(key.to_string(), (entry.content_type.clone(), data.clone()));
                return Some((entry.content_type, data));
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let entry_result = Self::get_idb_data(self, JsValue::from_str(key)).await;
            match entry_result {
                Ok(Some(entry)) => {
                    let data = Bytes::from(entry.data);
                    map.put(key.to_string(), (entry.content_type.clone(), data.clone()));
                    return Some((entry.content_type, data));
                }
                Ok(None) => return None,
                Err(_) => return None,
            };
        }

        None
    }

    pub fn insert(&self, key: String, content_type: String, bytes: Bytes) {
        let mut map = self.map.lock()?;
        map.put(key.clone(), (content_type.clone(), bytes.clone()));

        let entry = DiskCacheEntry {
            id: key.clone(),
            content_type,
            data: bytes.to_vec(),
        };

        #[cfg(not(target_arch = "wasm32"))]
        {
            let filename = Self::disk_path_for(&key);
            let bytes = serde_json::to_vec(&entry).unwrap();
            let _guard = self.file_lock.lock().unwrap();
            let _ = fs::write(filename, bytes);
        }

        #[cfg(target_arch = "wasm32")]
        {
            let insert_result = Self::insert_idb_data(self, entry).await;
            if let Err(err) = insert_result {
                log::error!("IndexedDB insert failed for {:?}: {:?}", key, err);
            }
        }
    }

    pub fn clear(&self) {
        let mut map = self.map.lock()?;
        map.clear();

        #[cfg(not(target_arch = "wasm32"))]
        {
            let _guard = self.file_lock.lock().unwrap();
            if Path::new(TILESET_CACHE_DIR).exists() {
                fs::remove_dir_all(TILESET_CACHE_DIR).ok();
            }
            fs::create_dir_all(TILESET_CACHE_DIR).ok();
        }

        #[cfg(target_arch = "wasm32")]
        {
            if let Some(db) = &self.db {
                let transaction = db
                    .transaction(&["abetterworld"], TransactionMode::ReadWrite)
                    .unwrap();
                let store = transaction.object_store("abetterworld").unwrap();
                store.clear().unwrap().await.unwrap();
                transaction.commit().unwrap().await.unwrap();
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn disk_path_for(key: &str) -> String {
        use hex;
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let hash = hasher.finalize();
        let encoded = hex::encode(hash);
        format!("{}/{}.json", TILESET_CACHE_DIR, encoded)
    }
}

use once_cell::sync::OnceCell;

#[cfg(not(target_arch = "wasm32"))]
pub static TILESET_CACHE: OnceCell<Arc<TilesetCache>> = OnceCell::new();

#[cfg(not(target_arch = "wasm32"))]
pub fn init_tileset_cache() -> Arc<TilesetCache> {
    let cache = Arc::new(TilesetCache::new());
    TILESET_CACHE
        .set(cache.clone())
        .expect("TILESET_CACHE was already initialized");
    cache
}

#[cfg(target_arch = "wasm32")]
use once_cell::unsync::OnceCell as LocalOnceCell;

#[cfg(target_arch = "wasm32")]
thread_local! {
    pub static TILESET_CACHE: LocalOnceCell<Arc<TilesetCache>> = LocalOnceCell::new();
}

#[cfg(target_arch = "wasm32")]
pub async fn init_tileset_cache() -> Arc<TilesetCache> {
    let cache = Arc::new(TilesetCache::new().await);
    TILESET_CACHE.with(|cell| {
        cell.set(cache.clone())
            .expect("TILESET_CACHE already initialized");
    });
    cache
}

pub fn get_tileset_cache() -> Option<Arc<TilesetCache>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        TILESET_CACHE.get().cloned()
    }
    #[cfg(target_arch = "wasm32")]
    {
        // In wasm, we use thread-local storage
        TILESET_CACHE.with(|cell| cell.get().cloned())
    }
}
