mod types;
pub use types::{DecodedMesh, OwnedDecodedMesh, Vertex};

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(target_arch = "wasm32")]
pub use wasm::DracoClient;

#[cfg(not(target_arch = "wasm32"))]
pub use native::DracoClient;
