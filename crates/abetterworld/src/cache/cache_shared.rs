use once_cell::sync::Lazy;
use std::sync::{Arc, Mutex};
use tracing::{event, Level};

use crate::cache::TilesetCache;

static TILESET_CACHE: Lazy<Mutex<Option<Arc<TilesetCache>>>> = Lazy::new(|| Mutex::new(None));

pub fn init_tileset_cache(cache_dir: &str) -> Arc<TilesetCache> {
    let mut guard = TILESET_CACHE.lock().unwrap();
    if let Some(existing) = guard.as_ref() {
        event!(Level::WARN, "TilesetCache already initialized");
        return existing.clone();
    }
    let cache = Arc::new(TilesetCache::new(cache_dir));
    *guard = Some(cache.clone());
    cache
}

pub fn get_tileset_cache() -> Arc<TilesetCache> {
    TILESET_CACHE
        .lock()
        .unwrap()
        .as_ref()
        .expect("TilesetCache not initialized")
        .clone()
}

pub fn destroy_tileset_cache() {
    let mut guard = TILESET_CACHE.lock().unwrap();
    *guard = None; // Drop the Arc
}
