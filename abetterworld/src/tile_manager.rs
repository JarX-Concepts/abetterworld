use crate::content::Tile;
use std::{collections::HashMap, sync::RwLock};

#[derive(Debug)]
pub struct TileManager {
    pub tileset: RwLock<HashMap<u64, Tile>>,
}

impl TileManager {
    pub fn new() -> Self {
        TileManager {
            tileset: RwLock::new(HashMap::new()),
        }
    }

    pub fn add_tile(&self, tile: Tile) {
        let add_this_tile = if let Ok(tileset_read) = self.tileset.read() {
            !tileset_read.contains_key(&tile.id)
        } else {
            false
        };
        if add_this_tile {
            let mut tileset = self.tileset.write().unwrap();
            tileset.insert(tile.id, tile);
        }
    }

    pub fn unload_tiles(&self, tiles: Vec<u64>) {
        if tiles.is_empty() {
            return;
        }
        let mut tileset = self.tileset.write().unwrap();
        for tile in tiles {
            tileset.remove(&tile);
        }
    }
}
