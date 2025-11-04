mod cache_shared;
mod types;

pub use cache_shared::{destroy_tileset_cache, get_tileset_cache, init_tileset_cache};

#[cfg(target_arch = "wasm32")]
mod cache_wasm;

#[cfg(target_arch = "wasm32")]
mod cache_lru_wasm;

#[cfg(target_arch = "wasm32")]
pub use cache_wasm::{init_wasm_indexdb_on_every_thread, TilesetCache};

#[cfg(not(target_arch = "wasm32"))]
mod cache_native;

#[cfg(not(target_arch = "wasm32"))]
mod cache_lru_native;

#[cfg(not(target_arch = "wasm32"))]
pub use cache_native::{init_wasm_indexdb_on_every_thread, TilesetCache};
