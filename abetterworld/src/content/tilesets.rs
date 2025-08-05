use crate::content::types::{Tile, TileState};
use crate::content::{download_content, BoundingVolume, Client, TileManager};
use crate::helpers::channel::Sender;
use crate::helpers::{hash_uri, AbwError, TileLoadingContext};
use crate::render::{Camera, CameraRefinementData};
use bytes::Bytes;
use cgmath::InnerSpace;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use url::Url;

pub const GOOGLE_API_KEY: &str = "AIzaSyD526Czd1rD44BZE2d2R70-fBEdDdf6vZQ";
pub const GOOGLE_API_URL: &str = "https://tile.googleapis.com/v1/3dtiles/root.json";

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

fn add_key_and_session(url: &str, key: &str, session: &Option<Arc<String>>) -> String {
    let mut url = Url::parse(url).unwrap();
    url.query_pairs_mut().append_pair("key", key);
    if let Some(session) = session {
        url.query_pairs_mut()
            .append_pair("session", session.as_str());
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

pub async fn parser_thread(
    cam: Arc<Camera>,
    tile_mgr: Arc<TileManager>,
    pager_tx: Sender<Tile>,
    client: Client,
    enable_sleep: bool,
) -> Result<(), AbwError> {
    let mut pager = TileSetImporter::new(client, pager_tx.clone(), tile_mgr);

    let mut last_cam_gen = 0;
    //loop
    {
        let new_gen = cam.generation();
        if new_gen != last_cam_gen {
            let camera_data = cam.refinement_data();

            if let Err(err) = pager.go(&camera_data, GOOGLE_API_URL, GOOGLE_API_KEY).await {
                log::error!("Failed to run pager: {}", err);
            }

            last_cam_gen = new_gen;
        } else {
            // No camera movement, sleep briefly to avoid busy-waiting
            if enable_sleep {
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
    Ok(())
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

    pub async fn go(
        &mut self,
        camera: &CameraRefinementData,
        url: &str,
        key: &str,
    ) -> Result<(), AbwError> {
        log::info!("Starting TileSetImporter for URL: {}", url);
        let ret = self.import_tileset(camera, url, key).await;

        log::info!("Finished TileSetImporter for URL: {}", url);

        // Compute tiles to unload
        let tiles_to_unload: Vec<u64> = self
            .last_pass_tiles
            .iter()
            .filter(|tile| !self.current_pass_tiles.contains(tile))
            .copied()
            .collect();

        log::info!("Finished tiles_to_unload: {}", url);

        // Unload the ones not in the current pass
        if !tiles_to_unload.is_empty() {
            self.tile_manager.mark_tiles_unload(tiles_to_unload);
        }

        log::info!("Finished mark_tiles_unload: {}", url);

        // Prepare for next pass
        self.last_pass_tiles = std::mem::take(&mut self.current_pass_tiles);
        self.current_pass_tiles.clear();

        log::info!("Finished last_pass_tiles: {}", url);

        ret
    }

    async fn import_tileset(
        &mut self,
        camera: &CameraRefinementData,
        url: &str,
        key: &str,
    ) -> Result<(), AbwError> {
        let (content_type, bytes) = self.download_content(url, key, &None).await?;

        match content_type.as_str() {
            "application/json" | "application/json; charset=UTF-8" => {
                let tileset: GltfTileset =
                    serde_json::from_slice(&bytes).tile_loading(&format!(
                        "Failed to parse tileset JSON: {}",
                        String::from_utf8_lossy(&bytes)
                    ))?;

                self.process_tileset(camera, tileset.root, url, key, None)
                    .await
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

    async fn process_tileset(
        &mut self,
        camera: &CameraRefinementData,
        root_tile: GltfTile,
        tileset_url: &str,
        key: &str,
        session: Option<Arc<String>>,
    ) -> Result<(), AbwError> {
        use std::collections::VecDeque;

        let mut stack = VecDeque::new();
        stack.push_back((root_tile, tileset_url.to_string(), session));

        while let Some((tile, base_url, session)) = stack.pop_back() {
            let mut added_geom = false;

            let needs_refinement = needs_refinement(
                camera,
                &tile.bounding_volume,
                tile.geometric_error,
                1024.0,
                100.0,
            );

            if let Some(content) = &tile.content {
                let tile_url = resolve_url(&base_url, &content.uri)?;
                let refine_mode = tile.refine.as_deref().unwrap_or("REPLACE");
                let new_session = extract_session(&tile_url);
                let current_session = match new_session {
                    Some(s) => match session.as_ref() {
                        Some(existing) if existing.as_str() == s => session.clone(),
                        _ => Some(Arc::new(s.to_string())),
                    },
                    None => session.clone(), // nothing new — reuse or continue None
                };

                if is_nested_tileset(&tile_url) {
                    let (content_type, bytes) = self
                        .download_content(&tile_url, key, &current_session)
                        .await?;

                    if content_type.starts_with("application/json") {
                        let tileset: GltfTileset =
                            serde_json::from_slice(&bytes).tile_loading(&format!(
                                "Failed to parse tileset JSON: {}",
                                String::from_utf8_lossy(&bytes)
                            ))?;
                        stack.push_back((tileset.root, tile_url.clone(), current_session));
                    }
                } else if is_glb(&tile_url)
                    && (refine_mode == "ADD" || tile.children.is_none() || !needs_refinement)
                {
                    added_geom = true;

                    let tile_url = add_key_and_session(&tile_url, key, &current_session);
                    let tile_id = hash_uri(&tile_url);

                    if !self.current_pass_tiles.contains(&tile_id) {
                        self.current_pass_tiles.insert(tile_id);

                        if !self.last_pass_tiles.contains(&tile_id) {
                            let new_tile = Tile {
                                counter: self.current_pass_tiles.len() as u64,
                                parent: None,
                                id: tile_id,
                                uri: tile_url,
                                volume: tile.bounding_volume.clone(),
                                state: TileState::ToLoad,
                            };

                            self.tile_manager.add_tile(&new_tile);

                            self.sender.send(new_tile).await.map_err(|_| {
                                AbwError::TileLoading("Failed to send new tile".into())
                            })?;
                        }
                    }
                }
            }

            if needs_refinement || !added_geom {
                if let Some(children) = &tile.children {
                    for child in children.iter().cloned() {
                        stack.push_back((child, base_url.clone(), session.clone()));
                    }
                }
            }
        }

        Ok(())
    }

    async fn download_content(
        &self,
        content_url: &str,
        key: &str,
        session: &Option<Arc<String>>,
    ) -> Result<(String, Bytes), AbwError> {
        download_content(&self.client, content_url, key, session).await
    }
}
