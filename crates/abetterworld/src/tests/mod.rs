#[cfg(not(target_arch = "wasm32"))]
mod cache;

#[cfg(target_arch = "wasm32")]
mod cache_wasm;

#[cfg(not(target_arch = "wasm32"))]
pub use cache::*;

#[cfg(target_arch = "wasm32")]
pub use cache_wasm::*;

#[cfg(not(target_arch = "wasm32"))]
mod paging;

mod dynamics;
