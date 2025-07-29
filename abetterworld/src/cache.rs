use bytes::Bytes;
use moka::sync::Cache;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(not(target_arch = "wasm32"))]
use std::{fs, path::Path, sync::RwLock};

#[cfg(target_arch = "wasm32")]
use idb::{Database, DatabaseEvent, Error, Factory, KeyPath, ObjectStoreParams, TransactionMode};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::wasm_bindgen::JsValue;

use crate::helpers::hash_uri;

const TILESET_CACHE_DIR: &str = "./tilesets";
const LRU_CACHE_CAPACITY: u64 = 512;

#[derive(Serialize, Deserialize)]
struct DiskCacheEntry {
    id: u64,
    content_type: String,
    data: Vec<u8>,
}

#[derive(Debug)]
pub struct TilesetCache {
    pub map: Cache<u64, (String, Bytes)>,
    file_lock: RwLock<()>,
    #[cfg(target_arch = "wasm32")]
    db: Option<Database>,
}

impl TilesetCache {
    pub fn new() -> Self {
        let map = Cache::new(LRU_CACHE_CAPACITY);

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
                file_lock: RwLock::new(()),
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
        let id = hash_uri(key);
        if let Some((ct, data)) = self.map.get(&id) {
            return Some((ct, data));
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let filename = Self::disk_path_for(key);
            if Path::new(&filename).exists() {
                let bytes = {
                    let _guard = self.file_lock.read().expect("cache get lock poisoned");
                    fs::read(&filename).ok()
                }?;
                let entry: DiskCacheEntry = serde_json::from_slice(&bytes).ok()?;
                let data = Bytes::from(entry.data.clone());
                self.map
                    .insert(id, (entry.content_type.clone(), data.clone()));
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
        let id = hash_uri(&key);
        self.map.insert(id, (content_type.clone(), bytes.clone()));

        let entry = DiskCacheEntry {
            id: id,
            content_type,
            data: bytes.to_vec(),
        };

        #[cfg(not(target_arch = "wasm32"))]
        {
            let filename = Self::disk_path_for(&key);
            let bytes = serde_json::to_vec(&entry).unwrap();
            let _guard = self.file_lock.write().expect("cache insert lock poisoned");
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
        self.map.invalidate_all();

        #[cfg(not(target_arch = "wasm32"))]
        {
            let _guard = self.file_lock.write().expect("cache clear lock poisoned");
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
