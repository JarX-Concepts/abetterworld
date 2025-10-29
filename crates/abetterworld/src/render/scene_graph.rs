use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{event, Level};

use crate::content::{TileInfo, TileKey, TileMessage};
use crate::helpers::AbwError;
use crate::render::{RenderTile, RenderableState};

pub type LockedRenderTilePtr = Arc<RwLock<RenderTile>>;
pub type RenderableMap = HashMap<TileKey, LockedRenderTilePtr>;

#[derive(Debug)]
pub struct SceneGraph {
    pub renderable: RenderableMap,
}

pub fn get_renderable_tile(
    renderables: &RenderableMap,
    key: TileKey,
) -> Result<LockedRenderTilePtr, AbwError> {
    renderables.get(&key).cloned().ok_or_else(|| {
        AbwError::Internal(format!("Renderable tile missing during render: {}", key))
    })
}

pub fn with_renderable_state<R>(
    renderables: &RenderableMap,
    key: TileKey,
    f: impl FnOnce(&RenderableState) -> R,
) -> Result<R, AbwError> {
    let ptr = renderables
        .get(&key)
        .ok_or_else(|| AbwError::Internal(format!("Missing: {}", key)))?;
    let guard = ptr.read().unwrap();
    let render_tile = guard
        .renderable_state
        .as_ref()
        .ok_or_else(|| AbwError::Internal(format!("RenderableState missing: {}", key)))?;
    Ok(f(render_tile)) // lock held only during f
}

impl SceneGraph {
    pub fn new() -> Self {
        SceneGraph {
            renderable: HashMap::new(),
        }
    }

    fn ensure_entry(&mut self, key: TileKey) -> &LockedRenderTilePtr {
        self.renderable.entry(key).or_insert_with(|| {
            event!(Level::DEBUG, key, "SceneGraph: inserting empty entry");
            Arc::new(RwLock::new(RenderTile {
                key,
                gen: 0,
                tile_info: None,
                renderable_state: None,
            }))
        })
    }

    pub fn add_info(&mut self, (msg, info): (TileMessage, TileInfo)) {
        let ptr = self.ensure_entry(msg.key).clone();
        let mut rt = ptr.write().expect("RenderTile RwLock poisoned");

        if msg.gen > rt.gen {
            event!(
                Level::TRACE,
                key = msg.key,
                old_gen = rt.gen,
                new_gen = msg.gen,
                "SceneGraph: info update (newer gen)"
            );
            rt.tile_info = Some(info);
            rt.gen = msg.gen;
        } else if msg.gen == rt.gen {
            event!(
                Level::WARN,
                key = msg.key,
                gen = msg.gen,
                "SceneGraph: duplicate info gen (unexpected)"
            );
        } else {
            event!(
                Level::WARN,
                key = msg.key,
                incoming_gen = msg.gen,
                current_gen = rt.gen,
                "SceneGraph: stale info ignored"
            );
        }
    }

    pub fn remove(&mut self, msg: TileMessage) {
        if let Some(ptr) = self.renderable.get(&msg.key) {
            let rt = ptr.read().expect("RenderTile RwLock poisoned");
            let current_max_gen = rt.gen.max(rt.gen);
            if msg.gen >= current_max_gen {
                drop(rt); // release read lock before removing
                event!(
                    Level::TRACE,
                    key = msg.key,
                    gen = msg.gen,
                    current_max_gen,
                    "SceneGraph: removing tile"
                );
                self.renderable.remove(&msg.key);
            } else {
                event!(
                    Level::WARN,
                    key = msg.key,
                    gen = msg.gen,
                    current_max_gen,
                    "SceneGraph: stale remove ignored"
                );
            }
        } else {
            event!(
                Level::DEBUG,
                key = msg.key,
                gen = msg.gen,
                "SceneGraph: remove for missing key (no-op)"
            );
        }
    }

    pub fn add_renderable(&mut self, tile_id: TileKey, renderable_state: RenderableState) {
        let ptr = self.ensure_entry(tile_id).clone();
        let mut rt = ptr.write().expect("RenderTile RwLock poisoned");
        event!(
            Level::TRACE,
            key = tile_id,
            "SceneGraph: set/update renderable"
        );
        rt.renderable_state = Some(renderable_state);
    }
}
