use crate::content::types::{Tile, TileState};
use crate::content::{download_content, BoundingVolume, Client, TileManager};
use crate::dynamics::{Camera, CameraRefinementData};
use crate::helpers::channel::Sender;
use crate::helpers::{hash_uri, AbwError, TileLoadingContext};
use crate::Source;
use bytes::Bytes;
use cgmath::{InnerSpace, Point3};
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
//use tracing::{event, span, Level};
use url::Url;

#[cfg(target_arch = "wasm32")]
use gloo_timers::future::TimeoutFuture;

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

fn is_nested_ext(uri: &str, ext: &str) -> bool {
    Url::parse(uri)
        .map(|url| url.path().ends_with(ext))
        .unwrap_or_else(|_| {
            uri.split('?')
                .next()
                .map_or(false, |path| path.ends_with(ext))
        })
}

fn is_nested_tileset(uri: &str) -> bool {
    is_nested_ext(uri, ".json")
}

fn is_glb(uri: &str) -> bool {
    is_nested_ext(uri, ".glb")
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

#[inline]
fn sphere_clearance_distance(eye: Point3<f64>, center: Point3<f64>, radius: f64) -> f64 {
    let d = (center - eye).magnitude() - radius.max(0.0);
    // Clamp to small positive to avoid division-by-zero; 0.5–2.0 m also fine.
    d.max(1e-2)
}

#[inline]
fn compute_sse(
    geometric_error: f64,
    viewport_height_px: f64,
    fovy_rad: f64,
    eye_to_surface_distance_m: f64,
) -> f64 {
    let denom = (fovy_rad * 0.5).tan() * 2.0;
    if !denom.is_finite() || denom <= 0.0 {
        return f64::INFINITY;
    }
    (geometric_error * viewport_height_px) / (denom * eye_to_surface_distance_m)
}

/// Drop-in `needs_refinement` using the 12-number box.
pub fn needs_refinement(
    camera: &CameraRefinementData,
    bv: &BoundingVolume, // <- your Google 12-number box
    geometric_error: f64,
    screen_height_pixels: f64, // pass *device* pixels if you render at DPR>1
    sse_threshold: f64,        // e.g., 16–30; start around 20
) -> bool {
    // 0) Sentinel/invalid GEs: refine to drill down
    if !geometric_error.is_finite() || geometric_error > 1.0e20 {
        return true;
    }

    // 1) Convert OBB -> bounding sphere
    let (center, radius) = bv.to_bounding_sphere();

    // 2) Clearance distance from eye to outside of sphere
    let dist = sphere_clearance_distance(camera.position, center, radius);

    // 3) FOV in radians
    let fovy_rad = camera.fovy.0.to_radians();

    // 4) Classic 3D Tiles SSE
    let sse = compute_sse(geometric_error, screen_height_pixels, fovy_rad, dist);

    sse > sse_threshold
}

pub async fn parser_iteration(
    source: &Source,
    camera_data: &CameraRefinementData,
    pager: &mut TileSetImporter,
) -> Result<(), AbwError> {
    match source {
        Source::Google { key, url } => pager.go(&camera_data, url, key).await,
        // Add more source types here as needed
        _ => {
            log::error!("Unsupported source type: {:?}", source);
            Err(AbwError::TileLoading("Unsupported source type".into()))
        }
    }
}

pub async fn parser_thread(
    source: &Source,
    cam: Arc<Camera>,
    tile_mgr: Arc<TileManager>,
    pager_tx: Sender<Tile>,
    client: Client,
    enable_sleep: bool,
) -> Result<(), AbwError> {
    let mut pager = TileSetImporter::new(client, pager_tx.clone(), tile_mgr);

    let mut last_cam_gen = 0;
    loop {
        //let span = span!(Level::TRACE, "pager pass");
        //let _enter = span.enter();

        let new_gen = cam.generation();
        if new_gen != last_cam_gen {
            let camera_data = cam.refinement_data();
            parser_iteration(source, &camera_data, &mut pager).await?;

            last_cam_gen = new_gen;
        } else {
            // No camera movement, sleep briefly to avoid busy-waiting
            if enable_sleep {
                thread::sleep(Duration::from_millis(10));
            }
        }

        //event!(Level::DEBUG, "something happened inside my_span");

        //drop(_enter);
        //drop(span);
    }

    Ok(())
}

impl TileSetImporter {
    pub fn new(client: Client, sender: Sender<Tile>, tile_manager: Arc<TileManager>) -> Self {
        Self {
            client,
            sender,
            tile_manager: tile_manager,
            current_pass_tiles: HashSet::new(),
        }
    }

    pub async fn go(
        &mut self,
        camera: &CameraRefinementData,
        url: &str,
        key: &str,
    ) -> Result<(), AbwError> {
        let ret = self.import_tileset(camera, url, key).await;

        self.tile_manager.keep_these_tiles(&self.current_pass_tiles);

        // Prepare for next pass
        self.current_pass_tiles.clear();

        ret
    }

    async fn import_tileset(
        &mut self,
        camera: &CameraRefinementData,
        url: &str,
        key: &str,
    ) -> Result<(), AbwError> {
        let (content_type, bytes) = self.download_content(url, key, &None).await?;

        // save the raw tileset for debugging
        if false {
            use std::fs;
            use std::path::Path;
            let filename = format!("data_debug/{}.json", hash_uri(url));
            if !Path::new(&filename).exists() {
                if let Err(e) = fs::create_dir_all("tilesets") {
                    eprintln!("Failed to create tilesets dir: {}", e);
                }
            }
            if let Err(e) = fs::write(&filename, &bytes) {
                eprintln!("Failed to write tileset debug file {}: {}", filename, e);
            } else {
                log::info!("Wrote tileset debug file: {}", filename);
            }
        }

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
        stack.push_back((root_tile, tileset_url.to_string(), session, None));

        while let Some((tile, base_url, session, parent_id)) = stack.pop_front() {
            let mut added_geom = false;

            let needs_refinement = needs_refinement(
                camera,
                &tile.bounding_volume,
                tile.geometric_error,
                1024.0,
                20.0,
            );

            let mut new_parent_id = None;

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
                        stack.push_back((
                            tileset.root,
                            tile_url.clone(),
                            current_session.clone(),
                            parent_id,
                        ));
                    }
                }

                if is_glb(&tile_url) {
                    added_geom = true;

                    let tile_url = add_key_and_session(&tile_url, key, &current_session);
                    let tile_id = hash_uri(&tile_url);
                    new_parent_id = Some(tile_id);

                    if !self.current_pass_tiles.contains(&tile_id) {
                        self.current_pass_tiles.insert(tile_id);

                        if !self.tile_manager.has_tile(tile_id) {
                            let new_tile = Tile {
                                counter: self.current_pass_tiles.len() as u64,
                                num_children: tile.children.as_ref().map_or(0, |c| c.len()),
                                parent: parent_id,
                                id: tile_id,
                                uri: tile_url,
                                volume: tile.bounding_volume.clone(),
                                state: TileState::ToLoad,
                            };

                            self.tile_manager.add_tile(&new_tile);

                            //log::info!("Added new tile: {:?}", new_tile);

                            self.sender.send(new_tile).await.map_err(|_| {
                                AbwError::TileLoading("Failed to send new tile".into())
                            })?;
                        }
                        // give up CPU time to other tasks
                        #[cfg(target_arch = "wasm32")]
                        TimeoutFuture::new(1).await;
                    }
                }
            }

            if needs_refinement || !added_geom {
                if let Some(children) = &tile.children {
                    for child in children.iter().cloned() {
                        stack.push_back((
                            child,
                            base_url.clone(),
                            session.clone(),
                            new_parent_id.clone(),
                        ));
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
