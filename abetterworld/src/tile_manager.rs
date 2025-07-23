use crate::content::Tile;
use std::{collections::HashMap, sync::RwLock};

#[derive(Debug)]
pub struct TileManager {
    pub tileset: RwLock<HashMap<String, Tile>>,
}

impl TileManager {
    pub fn new() -> Self {
        TileManager {
            tileset: RwLock::new(HashMap::new()),
        }
    }

    pub fn add_tile(&self, tile: Tile) {
        let mut tileset = self.tileset.write().unwrap();
        tileset.insert(tile.uri.clone(), tile);
    }

    pub fn unload_tiles(&self, tiles: Vec<u64>) {
        if tiles.is_empty() {
            return;
        }
        let mut tileset = self.tileset.write().unwrap();
        for tile in tiles {
            tileset.remove(&tile.to_string());
        }
    }
}
