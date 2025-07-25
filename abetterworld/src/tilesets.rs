use crate::camera::CameraRefinementData;
use crate::content::{Tile, TileState};
use crate::download::download_content;
use crate::errors::{AbwError, TileLoadingContext};
use crate::helpers::hash_uri;
use crate::tile_manager::TileManager;
use crate::volumes::BoundingVolume;
use bytes::Bytes;
use cgmath::InnerSpace;
use crossbeam_channel::Sender;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use url::Url;

#[derive(Debug, Deserialize)]
struct GltfTileset {
    root: GltfTile,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GltfTileContent {
    uri: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GltfTile {
    #[serde(rename = "boundingVolume")]
    bounding_volume: BoundingVolume,
    #[serde(rename = "geometricError")]
    geometric_error: f64,
    refine: Option<String>,
    content: Option<GltfTileContent>,
    children: Option<Vec<GltfTile>>,
}

#[derive(Debug)]
pub struct TileSetImporter {
    client: Client,
    sender: Sender<Tile>,
    tile_manager: Arc<TileManager>,
    last_pass_tiles: HashSet<u64>,
    current_pass_tiles: HashSet<u64>,
}

// This is not optimal (make a custom implementation that doesn't allocate extra strings)
fn resolve_url(base: &str, relative: &str) -> Result<String, AbwError> {
    use url::Url;
    let mut base_url = Url::parse(base).tile_loading("invalid base url")?;
    base_url.set_query(None);
    Ok(if relative.starts_with("http") {
        relative.to_string()
    } else {
        base_url
            .join(relative)
            .tile_loading("Failed to join base url")?
            .to_string()
    })
}

fn is_nested_tileset(uri: &str) -> bool {
    Url::parse(uri)
        .map(|url| url.path().ends_with(".json"))
        .unwrap_or_else(|_| {
            uri.split('?')
                .next()
                .map_or(false, |path| path.ends_with(".json"))
        })
}

fn is_glb(uri: &str) -> bool {
    uri.trim_end_matches('/').ends_with(".glb")
}

fn extract_session(url: &str) -> Option<&str> {
    url.split_once("session=").map(|(_, session)| session)
}

fn add_key_and_session(url: &str, key: &str, session: Option<&str>) -> String {
    let mut url = Url::parse(url).unwrap();
    url.query_pairs_mut().append_pair("key", key);
    if let Some(session) = session {
        url.query_pairs_mut().append_pair("session", session);
    }
    url.to_string()
}

fn needs_refinement(
    camera: &CameraRefinementData,
    bounding_volume: &BoundingVolume,
    geometric_error: f64,
    screen_height: f64,
    sse_threshold: f64,
) -> bool {
    if !geometric_error.is_finite() || geometric_error > 1e20 {
        return true; // Always refine root/sentinel
    }

    let obb = bounding_volume.to_obb();
    let cam_pos = camera.position;
    let closest_point = obb.closest_point(cam_pos);

    let is_inside = (closest_point - cam_pos).magnitude() < f64::EPSILON;
    let dist = if is_inside {
        0.0
    } else {
        let diagonal = obb.half_axes.iter().map(|a| a.magnitude()).sum::<f64>() * 2.0;
        (closest_point - cam_pos).magnitude().max(diagonal * 0.01)
    };

    if dist > camera.far {
        //return false; // far away, no need to refine
    }

    // 3. Compute vertical FOV (in radians)
    let vertical_fov = camera.fovy.0.to_radians();

    // 4. SSE formula
    let sse = (geometric_error * screen_height) / (dist * (vertical_fov * 0.5).tan() * 2.0);

    // 5. Needs refinement?
    sse > sse_threshold
}

impl TileSetImporter {
    pub fn new(client: Client, sender: Sender<Tile>, tile_manager: Arc<TileManager>) -> Self {
        Self {
            client,
            sender,
            tile_manager: tile_manager,
            last_pass_tiles: HashSet::new(),
            current_pass_tiles: HashSet::new(),
        }
    }

    pub fn go(
        &mut self,
        camera: &CameraRefinementData,
        url: &str,
        key: &str,
    ) -> Result<(), AbwError> {
        let ret = self.import_tileset(camera, url, key, None);

        // Compute tiles to unload
        let tiles_to_unload: Vec<u64> = self
            .last_pass_tiles
            .iter()
            .filter(|tile| !self.current_pass_tiles.contains(tile))
            .copied()
            .collect();

        // Unload the ones not in the current pass
        if !tiles_to_unload.is_empty() {
            self.tile_manager.unload_tiles(tiles_to_unload);
        }

        // Prepare for next pass
        self.last_pass_tiles = std::mem::take(&mut self.current_pass_tiles);
        self.current_pass_tiles.clear();

        ret
    }

    fn import_tileset(
        &mut self,
        camera: &CameraRefinementData,
        url: &str,
        key: &str,
        session: Option<&str>,
    ) -> Result<(), AbwError> {
        let (content_type, bytes) = self.download_content(url, key, session)?;

        match content_type.as_str() {
            "application/json" | "application/json; charset=UTF-8" => {
                let tileset: GltfTileset =
                    serde_json::from_slice(&bytes).tile_loading(&format!(
                        "Failed to parse tileset JSON: {}",
                        String::from_utf8_lossy(&bytes)
                    ))?;

                self.process_tileset(camera, &tileset.root, url, key, session)
            }
            _ => Err(AbwError::TileLoading(
                format!(
                    "Unsupported content type: {} for {} ({})",
                    content_type, url, key
                )
                .into(),
            )),
        }
    }

    fn process_tileset(
        &mut self,
        camera: &CameraRefinementData,
        tile_info: &GltfTile,
        tileset_url: &str,
        key: &str,
        session: Option<&str>,
    ) -> Result<(), AbwError> {
        let mut added_geom = false;

        let needs_refinement = needs_refinement(
            camera,
            &tile_info.bounding_volume,
            tile_info.geometric_error,
            1024.0,
            100.0,
        );

        if let Some(content) = &tile_info.content {
            let tile_url = resolve_url(&tileset_url, &content.uri)?;
            let refine_mode = tile_info.refine.as_deref().unwrap_or("REPLACE");
            let new_session = extract_session(&tile_url).or_else(|| session);

            if is_nested_tileset(&tile_url) {
                self.import_tileset(camera, &tile_url, key, new_session)?;
            } else if is_glb(&tile_url)
                && (refine_mode == "ADD" || tile_info.children.is_none() || !needs_refinement)
            {
                added_geom = true;

                // add key and session to the tile_url
                let tile_url = add_key_and_session(&tile_url, key, new_session);
                let tile_id = hash_uri(&tile_url);

                if !self.current_pass_tiles.contains(&tile_id) {
                    self.current_pass_tiles.insert(tile_id);

                    if !self.last_pass_tiles.contains(&tile_id) {
                        let new_tile = Tile {
                            counter: self.current_pass_tiles.len() as u64,
                            parent: None,
                            id: tile_id,
                            uri: tile_url,
                            session: None,
                            volume: tile_info.bounding_volume,
                            state: TileState::ToLoad,
                        };

                        //self.tile_manager.add_tile(new_tile);

                        // off you go; good luck; god speed
                        self.sender
                            .send(new_tile)
                            .map_err(|e| AbwError::TileLoading(e.to_string()))?;
                    }
                }
            } else if !is_glb(&tile_url) {
                // these are weird
                // https://tile.googleapis.com/v1/3dtiles/datasets/CgIYAQ/files/AJVsH2xhxJPWKbFgSv4QaTrl7SbTaFlJnvfES7rtU4UHj6Lt5ys_EykyPb_P6NdvvMm8XTjWA6bKUyTq94uFkec53CIZF33frCoSLMBSQiOnIlPsKc0G8BsSlYvL.glb?session=CPecuaT_6-PRSBDl8uHDBg
                // log::info!("What is this tile {}", tile_url);
            }
        }

        if needs_refinement || !added_geom {
            if let Some(children) = &tile_info.children {
                for child in children {
                    self.process_tileset(camera, &child, &tileset_url, &key, session)?;
                }
            }
        }

        Ok(())
    }

    fn download_content(
        &self,
        content_url: &str,
        key: &str,
        session: Option<&str>,
    ) -> Result<(String, Bytes), AbwError> {
        download_content(&self.client, content_url, key, session)
    }
}
