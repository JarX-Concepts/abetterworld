use crate::cache::types::TilesetMemoryCache;
use crate::helpers::{hash_uri, IoContext};
use crate::{cache::cache_lru_wasm::WasmCache, helpers::AbwError};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use tracing::{event, Level};

use {
    idb::{Database, DatabaseEvent, Factory, KeyPath, ObjectStoreParams, TransactionMode},
    std::cell::RefCell,
    wasm_bindgen::JsValue,
};

#[derive(Serialize, Deserialize)]
struct DiskCacheEntry {
    id: String,
    content_type: String,
    data: Vec<u8>,
}

pub struct TilesetCache {
    pub map: Arc<dyn TilesetMemoryCache>,
    file_lock: RwLock<()>,
}

impl TilesetCache {
    pub fn new(_cache_dir: &str) -> Self {
        let map: Arc<dyn TilesetMemoryCache> = Arc::new(WasmCache::new());

        Self {
            map,
            file_lock: RwLock::new(()),
        }
    }

    async fn get_idb_data(
        database: &Arc<Database>,
        id: JsValue,
    ) -> Result<Option<DiskCacheEntry>, AbwError> {
        let transaction = database
            .transaction(&["abetterworld"], TransactionMode::ReadOnly)
            .map_err(|e| AbwError::Io(format!("Failed to create transaction: {e:?}")))?;

        let store = transaction
            .object_store("abetterworld")
            .map_err(|e| AbwError::Io(format!("Failed to get object store: {e:?}")))?;

        let js_value_opt: Option<JsValue> = store
            .get(id)
            .io("Failed to get data from object store")?
            .await
            .io("Failed to await get operation")?;

        let entry = match js_value_opt {
            Some(js_val) => {
                let parsed = serde_wasm_bindgen::from_value(js_val).map_err(|e| {
                    AbwError::Io(format!("Failed to deserialize cache entry: {e:?}"))
                })?;
                Some(parsed)
            }
            None => None,
        };

        transaction.await.io("Failed to await transaction")?;

        Ok(entry)
    }

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
            .put(&js_value, None)
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

    pub async fn get(&self, key: &str) -> Result<Option<(String, Bytes)>, AbwError> {
        let id = hash_uri(key);
        if let Some((ct, data)) = self.map.get(id) {
            return Ok(Some((ct, data)));
        }

        let db_arc_result = IDB_DB.with(|cell| cell.borrow().clone());
        let db_arc = match db_arc_result {
            Some(db) => db,
            None => {
                event!(Level::WARN, "IndexedDB not initialized");
                return Ok(None);
            }
        };

        let entry_result = Self::get_idb_data(&db_arc, JsValue::from_str(&id.to_string())).await;
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

        let db_arc_result = IDB_DB.with(|cell| cell.borrow().clone());
        let db_arc = match db_arc_result {
            Some(db) => db,
            None => {
                event!(Level::WARN, "IndexedDB not initialized");
                return Ok(());
            }
        };

        let insert_result = Self::insert_idb_data(&db_arc, entry).await;
        if let Err(err) = insert_result {
            event!(
                Level::ERROR,
                "IndexedDB insert failed for {:?}: {:?}",
                key,
                err
            );
        }

        Ok(())
    }

    pub async fn clear(&self) -> Result<(), AbwError> {
        self.map.invalidate_all();
        let db_arc_result = IDB_DB.with(|cell| cell.borrow().clone());
        let db_arc = match db_arc_result {
            Some(db) => db,
            None => {
                event!(Level::WARN, "IndexedDB not initialized");
                return Ok(());
            }
        };

        let transaction = db_arc
            .transaction(&["abetterworld"], TransactionMode::ReadWrite)
            .unwrap();
        let store = transaction.object_store("abetterworld").unwrap();

        store.clear().unwrap().await.unwrap();
        transaction.commit().unwrap().await.unwrap();

        Ok(())
    }
}

thread_local! {
    static IDB_DB: RefCell<Option<Arc<Database>>> = RefCell::new(None);
}

pub async fn init_wasm_indexdb_on_every_thread() -> Result<(), AbwError> {
    event!(Level::INFO, "Loading IndexedDB...");

    let factory =
        Factory::new().map_err(|_| AbwError::Internal("Failed to create factory".into()))?;

    let mut open_request = factory
        .open("abetterworld", Some(1))
        .map_err(|e| AbwError::Internal(format!("open() failed: {e:?}")))?;

    open_request.on_upgrade_needed(|event| {
        if let Ok(db) = event.database() {
            let mut params = ObjectStoreParams::new();
            params.auto_increment(true);
            params.key_path(Some(KeyPath::new_single("id")));
            if !db.store_names().iter().any(|n| n == "abetterworld") {
                db.create_object_store("abetterworld", params);
            }
        } else {
            event!(Level::ERROR, "Upgrade needed but could not access DB");
        }
    });

    let db = open_request
        .await
        .map_err(|e| AbwError::Internal(format!("IndexedDB open_request failed: {e:?}")))?;

    IDB_DB.with(|cell| {
        *cell.borrow_mut() = Some(Arc::new(db));
    });

    event!(Level::INFO, "Done Loading IndexedDB...");
    Ok(())
}
