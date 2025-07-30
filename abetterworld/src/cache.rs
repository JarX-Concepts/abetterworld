use crate::errors::IoContext;
use bytes::Bytes;
use moka::sync::Cache;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock, RwLock};

#[cfg(not(wasm))]
use std::{fs, path::Path};

#[cfg(wasm)]
use {
    idb::{Database, DatabaseEvent, Factory, KeyPath, ObjectStoreParams, TransactionMode},
    std::cell::RefCell,
    wasm_bindgen::JsValue,
    wasm_bindgen_futures::spawn_local,
};

#[cfg(wasm)]
use crate::errors::AbwError;
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
}

impl TilesetCache {
    pub fn new() -> Self {
        let map = Cache::new(LRU_CACHE_CAPACITY);

        #[cfg(not(wasm))]
        fs::create_dir_all(TILESET_CACHE_DIR).ok();

        return Self {
            map,
            file_lock: RwLock::new(()),
        };
    }

    #[cfg(wasm)]
    async fn get_idb_data(
        database: &Arc<Database>,
        id: JsValue,
    ) -> Result<Option<DiskCacheEntry>, AbwError> {
        // Create a read-only transaction
        let transaction = database
            .transaction(&["abetterworld"], TransactionMode::ReadOnly)
            .unwrap();

        // Get the object store
        let store = transaction.object_store("abetterworld").unwrap();

        // Get the stored data
        let stored_employee: Option<JsValue> = store
            .get(id)
            .io("Failed to get data from object store")?
            .await
            .io("Failed to await get operation")?;

        // Deserialize the stored data
        let stored_employee: Option<DiskCacheEntry> = stored_employee
            .map(|stored_employee| serde_wasm_bindgen::from_value(stored_employee).unwrap());

        // Wait for the transaction to complete (alternatively, you can also commit the transaction)
        transaction.await.io("Failed to await transaction")?;

        Ok(stored_employee)
    }

    #[cfg(wasm)]
    async fn insert_idb_data(
        database: &Database,
        entry: DiskCacheEntry,
    ) -> Result<JsValue, AbwError> {
        let transaction = database
            .transaction(&["abetterworld"], TransactionMode::ReadWrite)
            .io("Failed to create transaction")?;

        // Get the object store
        let store = transaction.object_store("abetterworld").unwrap();

        // Serialize the entry to JsValue using serde_wasm_bindgen
        let js_value = serde_wasm_bindgen::to_value(&entry).unwrap();

        // Add data to object store
        let id = store
            .add(&js_value, None)
            .unwrap()
            .await
            .io("Failed to add data to object store")?;

        // Commit the transaction
        transaction
            .commit()
            .io("Failed to commit transaction")?
            .await
            .io("Failed to commit transaction")?;

        Ok(id)
    }

    #[cfg(not(wasm))]
    pub fn get(&self, key: &str) -> Option<(String, Bytes)> {
        let id = hash_uri(key);
        if let Some((ct, data)) = self.map.get(&id) {
            return Some((ct, data));
        }

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

        None
    }

    #[cfg(wasm)]
    pub async fn get(&self, key: &str) -> Result<Option<(String, Bytes)>, AbwError> {
        let id = hash_uri(key);
        if let Some((ct, data)) = self.map.get(&id) {
            return Ok(Some((ct, data)));
        }

        let db_arc = IDB_DB
            .with(|cell| cell.borrow().clone())
            .ok_or_else(|| AbwError::Internal("IndexedDB not initialized".to_string()))?;

        let entry_result = Self::get_idb_data(&db_arc, JsValue::from_str(key)).await;
        match entry_result {
            Ok(Some(entry)) => {
                let data = Bytes::from(entry.data);
                self.map
                    .insert(id, (entry.content_type.clone(), data.clone()));
                return Ok(Some((entry.content_type, data)));
            }
            Ok(None) => return Ok(None),
            Err(_) => return Err(AbwError::Io("Failed to get data from IndexedDB".to_owned())),
        };
    }

    pub fn insert(&self, key: String, content_type: String, bytes: Bytes) {
        let id = hash_uri(&key);
        self.map.insert(id, (content_type.clone(), bytes.clone()));

        let entry = DiskCacheEntry {
            id: id,
            content_type,
            data: bytes.to_vec(),
        };

        #[cfg(not(wasm))]
        {
            let filename = Self::disk_path_for(&key);
            let bytes = serde_json::to_vec(&entry).unwrap();
            let _guard = self.file_lock.write().expect("cache insert lock poisoned");
            let _ = fs::write(filename, bytes);
        }

        #[cfg(wasm)]
        {
            with_db(|db| {
                let db_cloned = db.clone();
                spawn_local(async move {
                    let insert_result = Self::insert_idb_data(&db_cloned, entry).await;
                    if let Err(err) = insert_result {
                        log::error!("IndexedDB insert failed for {:?}: {:?}", key, err);
                    }
                });
            });
        }
    }

    pub fn clear(&self) -> Result<(), AbwError> {
        self.map.invalidate_all();

        #[cfg(not(wasm))]
        {
            let _guard = self.file_lock.write().expect("cache clear lock poisoned");
            if Path::new(TILESET_CACHE_DIR).exists() {
                fs::remove_dir_all(TILESET_CACHE_DIR).ok();
            }
            fs::create_dir_all(TILESET_CACHE_DIR).ok();
        }

        #[cfg(wasm)]
        {
            with_db(|database| {
                let transaction = database
                    .transaction(&["abetterworld"], TransactionMode::ReadWrite)
                    .unwrap();
                let store = transaction.object_store("abetterworld").unwrap();
                spawn_local(async move {
                    store.clear().unwrap().await.unwrap();
                    transaction.commit().unwrap().await.unwrap();
                });
            });
        }
        Ok(())
    }

    #[cfg(not(wasm))]
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

#[cfg(target_arch = "wasm32")]
thread_local! {
    static IDB_DB: RefCell<Option<Arc<Database>>> = RefCell::new(None);
}

#[cfg(target_arch = "wasm32")]
pub fn load_db() {
    spawn_local(async {
        let factory = Factory::new().expect("Failed to create IndexedDB factory");
        let mut open_request = factory.open("abetterworld", Some(1)).unwrap();

        open_request.on_upgrade_needed(|event| {
            let db = event.database().unwrap();
            let mut params = ObjectStoreParams::new();
            params.auto_increment(true);
            params.key_path(Some(KeyPath::new_single("id")));
            if !db.store_names().iter().any(|n| n == "abetterworld") {
                db.create_object_store("abetterworld", params);
            }
        });

        match open_request.await {
            Ok(database) => {
                IDB_DB.with(|cell| {
                    *cell.borrow_mut() = Some(Arc::new(database));
                });
            }
            Err(e) => {
                log::error!("Failed to open IndexedDB: {:?}", e);
            }
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn with_db<F, R>(f: F) -> Result<R, AbwError>
where
    F: FnOnce(&Arc<Database>) -> R,
{
    IDB_DB.with(|cell| {
        let maybe_arc = cell.borrow();
        let db = maybe_arc
            .as_ref()
            .ok_or_else(|| AbwError::Internal("IndexedDB not initialized".to_string()))?;
        Ok(f(db))
    })
}
