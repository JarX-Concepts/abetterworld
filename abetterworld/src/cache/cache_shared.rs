use crate::cache::TilesetCache;
use std::sync::{Arc, OnceLock};

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
