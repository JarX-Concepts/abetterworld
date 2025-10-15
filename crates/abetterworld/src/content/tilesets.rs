use crate::content::tiles_priority::{priortize, Pri};
use crate::content::types::Tile;
use crate::content::{download_content, BoundingVolume, Client};
use crate::dynamics::{Camera, CameraRefinementData};
use crate::helpers::channel::Sender;
use crate::helpers::{hash_uri, AbwError, TileLoadingContext};
use crate::Source;
use cgmath::{InnerSpace, Point3};
use serde::Deserialize;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use url::Url;
use wasm_bindgen_futures::spawn_local;

#[derive(Debug, Deserialize, Default, Clone)]
struct TileSourceRoot {
    pub root: Option<TileSource>,
}

#[derive(Debug, Default, Clone)]
struct TileSourceRootShared {
    pub root: Option<TileSource>,
    pub done: bool,
}

#[derive(Debug, Clone)]
pub enum TileSourceContentState {
    ToLoadVisual,
    LoadedTileSet {
        shared: Arc<RwLock<TileSourceRootShared>>,
        permanent: Option<Box<TileSourceRoot>>,
    },
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct TileSourceContent {
    pub uri: String,

    #[serde(skip, default)]
    pub key: Option<String>,

    #[serde(skip, default)]
    pub session: Option<String>,

    #[serde(skip, default)]
    pub loaded: Option<TileSourceContentState>,

    #[serde(skip, default)]
    pub id: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TileSource {
    #[serde(rename = "boundingVolume")]
    pub bounding_volume: BoundingVolume,
    #[serde(rename = "geometricError")]
    pub geometric_error: f64,
    pub refine: Option<String>,
    pub content: Option<TileSourceContent>,
    pub children: Option<Vec<TileSource>>,

    #[serde(skip, default)]
    // None  -> do not render;
    // true  -> render, but we might have better stuff;
    // false -> render, no better stuff
    pub needs_refinement_flag: Option<bool>,
}

// Has the pager sent this tile into the system?
enum TilePipelineMessage {
    Load(Tile),
    Unload(u64),
}
pub type TilePipelineState = Vec<u64>;

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

fn is_visual(uri: &str) -> bool {
    is_nested_ext(uri, ".glb")
}

fn extract_session(url: &str) -> Option<&str> {
    url.split_once("session=").map(|(_, session)| session)
}

fn add_key_and_session(url: &str, key: &str, session: &Option<String>) -> String {
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
fn needs_refinement(
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

fn load_tile(client: &Client, key: &String, tile: &mut TileSourceContent) -> Result<(), AbwError> {
    tile.id = hash_uri(&tile.uri);

    if is_nested_tileset(&tile.uri) {
        let tile_dst = Arc::new(RwLock::new(TileSourceRootShared::default()));
        tile.loaded = Some(TileSourceContentState::LoadedTileSet {
            shared: tile_dst.clone(),
            permanent: None,
        });

        let client = client.clone();
        let key = key.clone();
        let uri = tile.uri.clone();
        let session = tile.session.clone();

        spawn_local({
            let tile_dst = tile_dst.clone();
            async move {
                match download_content(&client, &uri, &key, &session).await {
                    Ok((content_type, bytes)) => {
                        if content_type.starts_with("application/json") {
                            match serde_json::from_slice::<TileSourceRoot>(&bytes) {
                                Ok(ts) => {
                                    *tile_dst.write().unwrap() = TileSourceRootShared {
                                        root: ts.root,
                                        done: true,
                                    }; // <- store it
                                }
                                Err(e) => {
                                    // TODO: log
                                }
                            }
                        } else {
                            // TODO: log unsupported content type
                        }
                    }
                    Err(e) => {
                        // TODO: log
                    }
                }
            }
        });

        return Ok(());
    } else if is_visual(&tile.uri) {
        // Just a visual tile (glb)
        tile.loaded = Some(TileSourceContentState::ToLoadVisual);

        return Ok(());
    }

    return Err(AbwError::TileLoading(
        format!("Unsupported file extension: {} ({})", tile.uri, key).into(),
    ));
}

fn build_child_tile_content(parent: &Option<&TileSourceContent>, tile: &mut TileSourceContent) {
    if let Some(parent) = parent {
        if let Ok(resolved) = resolve_url(&parent.uri, &tile.uri) {
            tile.uri = resolved;
        }
        if tile.key.is_none() {
            tile.key = parent.key.clone();
        }

        let new_session = extract_session(&tile.uri);
        if let Some(session) = new_session {
            tile.session = Some(session.to_string());
        } else if tile.session.is_none() {
            tile.session = parent.session.clone();
        }
    }
}

fn process_tile_content(
    source: &Source,
    client: &Client,
    camera: &CameraRefinementData,
    parent: &Option<&TileSourceContent>,
    tile: &mut TileSourceContent,
) -> Result<(), AbwError> {
    if tile.loaded.is_none() {
        build_child_tile_content(parent, tile);

        load_tile(
            client,
            match source {
                Source::Google { key, .. } => key,
                _ => return Err(AbwError::TileLoading("Unsupported source type".into())),
            },
            tile,
        )?;
    } else {
        match tile.loaded.as_mut() {
            Some(TileSourceContentState::ToLoadVisual) => {}
            Some(TileSourceContentState::LoadedTileSet { shared, permanent }) => {
                if let Some(permanent_root) = permanent.as_mut() {
                    if let Some(root) = &mut permanent_root.root {
                        process_tile(source, client, camera, parent, root)?;
                    }
                } else {
                    let read_guard = shared.read().unwrap();
                    if read_guard.done {
                        // move a copy into permanent now that loading is done
                        *permanent = Some(Box::new(TileSourceRoot {
                            root: read_guard.root.clone(),
                        }));
                    }
                }
            }
            None => {}
        }
    }

    Ok(())
}

pub fn force_refinement(tile: &mut TileSource, flag: Option<bool>, skip_parent: bool) {
    if !skip_parent {
        tile.needs_refinement_flag = flag;
    }

    if let Some(content) = &mut tile.content {
        match content.loaded.as_mut() {
            Some(TileSourceContentState::LoadedTileSet { permanent, .. }) => {
                if let Some(permanent_root) = permanent.as_mut() {
                    if let Some(root) = &mut permanent_root.root {
                        force_refinement(root, flag, false);
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(children) = tile.children.as_mut() {
        for child in children {
            force_refinement(child, flag, false);
        }
    }
}

pub fn process_tile(
    source: &Source,
    client: &Client,
    camera: &CameraRefinementData,
    parent: &Option<&TileSourceContent>,
    tile: &mut TileSource,
) -> Result<(), AbwError> {
    let needs_refinement = needs_refinement(
        camera,
        &tile.bounding_volume,
        tile.geometric_error,
        camera.screen_height,
        camera.sse_threshold,
    );

    tile.needs_refinement_flag = Some(needs_refinement);

    if needs_refinement {
        match &mut tile.content {
            Some(content) => process_tile_content(source, client, camera, parent, content)?,
            None => {}
        }
        if let Some(children) = tile.children.as_mut() {
            for child in children {
                process_tile(source, client, camera, parent, child)?;
            }
        }
    } else {
        force_refinement(tile, None, true);
    }

    Ok(())
}

pub fn go(
    source: &Source,
    client: &Client,
    camera: &CameraRefinementData,
    root: &mut Option<TileSourceContent>,
) -> Result<(), AbwError> {
    if root.is_none() {
        match source {
            Source::Google { key, url } => {
                *root = Some(TileSourceContent {
                    uri: url.clone(),
                    key: Some(key.clone()),
                    session: None,
                    loaded: None,
                    id: hash_uri(url),
                });
            }
            _ => {
                return Err(AbwError::TileLoading("Unsupported source type".into()));
            }
        }
    }

    let mut tile = root.as_mut().unwrap();
    process_tile_content(source, client, camera, &None, &mut tile)
}

pub fn send_load_tile(
    tile_src: &TileSource,
    tile_content: &TileSourceContent,
    pager_tx: &mut Sender<TilePipelineMessage>,
) -> Result<(), AbwError> {
    let tile = Tile {
        id: tile_content.id,
        uri: tile_content.uri.clone(),
        state: crate::content::types::TileState::ToLoad,
        num_children: todo!(),
        parent: todo!(),
        volume: tile_src.bounding_volume,
    };
    pager_tx.try_send(TilePipelineMessage::Load(tile))
}

pub fn send_unload_tile(
    id: u64,
    pager_tx: &mut Sender<TilePipelineMessage>,
) -> Result<(), AbwError> {
    pager_tx.try_send(TilePipelineMessage::Unload(id))
}

pub fn parser_iteration(
    source: &Source,
    client: &Client,
    camera_data: &CameraRefinementData,
    root: &mut Option<TileSourceContent>,
    pipeline_state: &mut TilePipelineState,
    pager_tx: &mut Sender<TilePipelineMessage>,
) -> Result<(), AbwError> {
    go(source, client, camera_data, root)?;

    if let Some(tile) = root {
        if let Some(TileSourceContentState::LoadedTileSet { permanent, .. }) = &tile.loaded {
            if let Some(permanent_root) = permanent.as_ref() {
                if let Some(root) = &permanent_root.root {
                    // gather priority tiles
                    let mut priority_list: Vec<Pri> = Vec::new();

                    priortize(camera_data, root, &mut priority_list)?;

                    // send as many as we can into the pipeline
                    for pri in priority_list.iter() {
                        if !pipeline_state.contains(&pri.tile_content.id) {
                            if let Err(_err) = send_load_tile(pri.tile, pri.tile_content, pager_tx)
                            {
                                // the channel is full, we will try again next time
                                break;
                            }

                            pipeline_state.push(pri.tile_content.id);
                        }
                    }

                    // need a list of tiles currently in the pipeline state, but not in the priority list
                    // these can be removed
                    let mut to_remove: Vec<u64> = Vec::new();
                    for tile_id in pipeline_state.iter() {
                        if !priority_list
                            .iter()
                            .any(|pri| pri.tile_content.id == *tile_id)
                        {
                            if let Err(_err) = send_unload_tile(*tile_id, pager_tx) {
                                // the channel is full, we will try again next time
                                break;
                            }

                            to_remove.push(*tile_id);
                        }
                    }
                    // remove the tiles from the pipeline state
                    pipeline_state.retain(|id| !to_remove.contains(id));
                }
            }
        }
    }
    Ok(())
}

pub fn parser_thread(
    source: &Source,
    cam: Arc<Camera>,
    pager_tx: &mut Sender<TilePipelineMessage>,
    client: Client,
    enable_sleep: bool,
) -> Result<(), AbwError> {
    let mut root = None;
    let mut pipeline_state = TilePipelineState::new();

    let mut last_cam_gen = 0;
    loop {
        //let span = span!(Level::TRACE, "pager pass");
        //let _enter = span.enter();

        let new_gen = cam.generation();
        if new_gen != last_cam_gen {
            let camera_data = cam.refinement_data();
            parser_iteration(
                source,
                &client,
                &camera_data,
                &mut root,
                &mut pipeline_state,
                pager_tx,
            )?;

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
