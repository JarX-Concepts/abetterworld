use crate::content::{RenderableState, Tile, TileState};
use std::{collections::HashMap, sync::RwLock};

#[derive(Debug)]
pub struct TileManager {
    pub tileset: RwLock<HashMap<u64, Tile>>,
    pub renderable: RwLock<HashMap<u64, RenderableState>>,
}

impl TileManager {
    pub fn new() -> Self {
        TileManager {
            tileset: RwLock::new(HashMap::new()),
            renderable: RwLock::new(HashMap::new()),
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
            renderable.insert(tile.tile.id, tile);
        }
    }

    pub fn mark_tiles_unload(&self, tiles: Vec<u64>) {
        if tiles.is_empty() {
            return;
        }

        let tile_ids: std::collections::HashSet<u64> = tiles.into_iter().collect();
        let mut tileset = self.tileset.write().unwrap();
        tileset.retain(|tile_id, _| !tile_ids.contains(tile_id));
    }

    pub fn unload_tiles(&self) {
        let tileset = self.tileset.read().unwrap();
        let mut renderable = self.renderable.write().unwrap();

        renderable.retain(|tile_id, _| tileset.contains_key(tile_id));
    }
}
