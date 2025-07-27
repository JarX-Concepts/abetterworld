use crate::content::{Tile, TileState};
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

    pub fn mark_tiles_unload(&self, tiles: Vec<u64>) {
        if tiles.is_empty() {
            return;
        }
        let mut tileset = self.tileset.write().unwrap();
        for tile in tiles {
            if let Some(existing_tile) = tileset.get_mut(&tile) {
                match &mut existing_tile.state {
                    TileState::Renderable { unload, .. } => {
                        // need to do this from the render thread
                        *unload = true; // mark as needing unload
                    }
                    _ => {
                        existing_tile.state = TileState::Unload;
                    }
                }
            }
        }
    }

    pub fn unload_tiles(&self) {
        let mut tileset = self.tileset.write().unwrap();
        tileset.retain(|_, tile| match &tile.state {
            TileState::Unload => false,
            TileState::Renderable { unload: true, .. } => false,
            _ => true,
        });
    }
}
