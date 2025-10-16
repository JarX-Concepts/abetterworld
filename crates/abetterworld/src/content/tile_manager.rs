use crate::content::types::{RenderableState, Tile};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

pub type RenderableMap = HashMap<u64, Arc<RenderableState>>;

#[derive(Debug)]
pub struct TileManager {
    pub tileset: RwLock<HashMap<u64, Tile>>,
    pub renderable: RwLock<RenderableMap>,
}

impl TileManager {
    pub fn new() -> Self {
        TileManager {
            tileset: RwLock::new(HashMap::new()),
            renderable: RwLock::new(HashMap::new()),
        }
    }

    pub fn has_tile(&self, id: u64) -> bool {
        if let Ok(tileset_read) = self.tileset.read() {
            tileset_read.contains_key(&id)
        } else {
            false
        }
    }

    pub fn add_tile(&self, tile: &Tile) {
        let add_this_tile = if let Ok(tileset_read) = self.tileset.read() {
            !tileset_read.contains_key(&tile.id)
        } else {
            false
        };
        if add_this_tile {
            let mut tileset = self.tileset.write().unwrap();
            tileset.insert(tile.id, tile.clone());
        }
    }

    pub fn add_renderable(&self, tile: RenderableState) {
        let add_this_tile = if let Ok(tileset_read) = self.renderable.read() {
            !tileset_read.contains_key(&tile.tile.id)
        } else {
            false
        };
        if add_this_tile {
            let mut renderable = self.renderable.write().unwrap();
            renderable.insert(tile.tile.id, tile.into());
        }
    }

    pub fn remove_renderable(&self, id: u64) {
        let mut renderable = self.renderable.write().unwrap();
        renderable.remove(&id);
    }
}
