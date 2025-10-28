use crate::content::{TileInfo, TileKey};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

pub type TileInfoState = HashMap<TileKey, Arc<TileInfo>>;
pub type TileContentState = Vec<TileKey>;

#[derive(Debug)]
pub struct TileManager {
    pub tile_info: RwLock<TileInfoState>,
    pub tile_content_loaded: RwLock<TileContentState>,
}

impl TileManager {
    pub fn new() -> Self {
        TileManager {
            tile_info: RwLock::new(HashMap::new()),
            tile_content_loaded: RwLock::new(Vec::new()),
        }
    }

    pub fn is_tile_loaded(&self, key: TileKey) -> bool {
        let tile_content = self.tile_content_loaded.read().unwrap();
        tile_content.contains(&key)
    }

    pub fn mark_tile_loaded(&self, key: TileKey) -> bool {
        let mut content_map = self.tile_content_loaded.write().unwrap();
        if !content_map.contains(&key) {
            content_map.push(key);
            return false;
        }
        true
    }

    pub fn mark_tile_unloaded(&self, key: TileKey) {
        let mut content_map = self.tile_content_loaded.write().unwrap();
        if let Some(pos) = content_map.iter().position(|x| *x == key) {
            content_map.swap_remove(pos);
        }
    }

    pub fn has_tile_info(&self, key: TileKey) -> bool {
        let tile_info = self.tile_info.read().unwrap();
        tile_info.contains_key(&key)
    }

    pub fn has_tile_with_children(&self, key: TileKey) -> bool {
        let tile_info = self.tile_info.read().unwrap();
        if let Some(info) = tile_info.get(&key) {
            return info.children.is_some() && !info.children.as_ref().unwrap().is_empty();
        }
        false
    }

    pub fn compare_tile_info(&self, key: TileKey, tile_info: &TileInfo) -> bool {
        let info_map = self.tile_info.read().unwrap();
        if let Some(existing) = info_map.get(&key) {
            return **existing == *tile_info;
        }
        false
    }

    pub fn add_or_update_tile_info(&self, key: TileKey, tile_info: TileInfo) -> bool {
        let mut info_map = self.tile_info.write().unwrap();
        let existing = info_map.insert(key, Arc::new(tile_info));
        existing.is_some()
    }

    pub fn remove_tile_info(&self, id: TileKey) {
        let mut info_map = self.tile_info.write().unwrap();
        info_map.remove(&id);
    }
}
