use crate::content::pager_state::{add_tile, child_count, upsert_child_count};
use crate::content::tiles_priority::{priortize, Pri};
use crate::content::{
    download_content, BoundingVolume, Client, Gen, TileContent, TileKey, TileManager, TileMessage,
    TilePipelineMessage,
};
use crate::dynamics::{Camera, CameraRefinementData};
use crate::helpers::channel::Sender;
use crate::helpers::{hash_uri, spawn_detached, AbwError, TileLoadingContext};
use crate::Source;
use cgmath::{InnerSpace, Point3};
use serde::Deserialize;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use tracing::{event, Level};
use url::Url;

#[derive(Debug, Deserialize, Default, Clone)]
pub struct TileSourceRoot {
    pub root: Option<TileSource>,
}

#[derive(Debug, Clone)]
pub struct TileSourceRootShared {
    pub root: Option<TileSource>,
    pub done: bool,
}

#[derive(Debug, Clone)]
pub enum TileSourceContentState {
    Visual,
    LoadingTileSet {
        shared: Arc<RwLock<TileSourceRootShared>>,
    },
    LoadedTileSet {
        permanent: Option<Box<TileSourceRoot>>,
    },
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct TileSourceContent {
    pub uri: String,

    #[serde(skip, default)]
    pub access_key: Option<String>,

    #[serde(skip, default)]
    pub session: Option<String>,

    #[serde(skip, default)]
    pub loaded: Option<TileSourceContentState>,

    #[serde(skip, default)]
    pub key: TileKey,
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
    pub needs_refinement_flag: Option<bool>,
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

fn is_visual(uri: &str) -> bool {
    is_nested_ext(uri, ".glb")
}

fn extract_session(url: &str) -> Option<&str> {
    url.split_once("session=").map(|(_, session)| session)
}

fn add_key_and_session(url: &str, key: &Option<String>, session: &Option<String>) -> String {
    let mut url = Url::parse(url).unwrap();

    if let Some(key) = key {
        url.query_pairs_mut().append_pair("key", key.as_str());
    }

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
    tile.key = hash_uri(&tile.uri);

    if is_nested_tileset(&tile.uri) {
        let tile_dst = Arc::new(RwLock::new(TileSourceRootShared {
            root: None,
            done: false,
        }));
        tile.loaded = Some(TileSourceContentState::LoadingTileSet {
            shared: tile_dst.clone(),
        });

        let client = client.clone();
        let uri = tile.uri.clone();

        spawn_detached({
            let tile_dst = tile_dst.clone();
            async move {
                match download_content(&client, &uri).await {
                    Ok((content_type, bytes)) => {
                        if content_type.starts_with("application/json") {
                            match serde_json::from_slice::<TileSourceRoot>(&bytes) {
                                Ok(ts) => {
                                    event!(Level::INFO, "Loaded tileset: {}", uri);

                                    *tile_dst.write().unwrap() = TileSourceRootShared {
                                        root: ts.root,
                                        done: true,
                                    }; // <- store it
                                }
                                Err(e) => {
                                    event!(Level::ERROR, "Failed to parse tileset JSON: {}", e);
                                }
                            }
                        } else {
                            event!(Level::ERROR, "Unsupported content type: {}", content_type);
                        }
                    }
                    Err(e) => {
                        event!(Level::ERROR, "Failed to download tile content: {}", e);
                    }
                }
            }
        });

        return Ok(());
    } else if is_visual(&tile.uri) {
        // Just a visual tile (glb)
        tile.loaded = Some(TileSourceContentState::Visual);

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
        if tile.access_key.is_none() {
            tile.access_key = parent.access_key.clone();
        }

        let new_session = extract_session(&tile.uri);
        if let Some(session) = new_session {
            tile.session = Some(session.to_string());
        } else if tile.session.is_none() {
            tile.session = parent.session.clone();
        }
    }
    tile.uri = add_key_and_session(&tile.uri, &tile.access_key, &tile.session);
}

fn process_tile_content(
    source: &Source,
    client: &Client,
    camera: &CameraRefinementData,
    tileset: &Option<&TileSourceContent>,
    tile_content: &mut TileSourceContent,
) -> Result<(), AbwError> {
    if tile_content.loaded.is_none() {
        build_child_tile_content(tileset, tile_content);

        return load_tile(
            client,
            match source {
                Source::Google { key, .. } => key,
                _ => return Err(AbwError::TileLoading("Unsupported source type".into())),
            },
            tile_content,
        );
    }

    // Move `loaded` out to avoid overlapping borrows of `tile_content`
    let mut loaded = tile_content.loaded.take();

    let new_loaded = match &mut loaded {
        Some(TileSourceContentState::LoadingTileSet { shared }) => {
            let guard = shared.read().expect("tileset shared lock poisoned");
            if guard.done {
                let new_loaded = Some(TileSourceContentState::LoadedTileSet {
                    permanent: Some(Box::new(TileSourceRoot {
                        root: guard.root.clone(),
                    })),
                });
                new_loaded
            } else {
                drop(guard);
                loaded
            }
        }

        Some(TileSourceContentState::LoadedTileSet { permanent }) => {
            // We should already have a permanent root, process it immediately

            if let Some(root) = permanent.as_mut().and_then(|p| p.root.as_mut()) {
                process_tile(source, client, camera, &Some(tile_content), root)?;
            }
            loaded
        }

        Some(TileSourceContentState::Visual { .. }) => loaded,

        _ => loaded,
    };

    // Put `loaded` back
    tile_content.loaded = new_loaded;

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
    tileset: &Option<&TileSourceContent>,
    tile: &mut TileSource,
) -> Result<(), AbwError> {
    match &mut tile.content {
        Some(content) => process_tile_content(source, client, camera, tileset, content)?,
        None => {}
    };

    let needs_refinement = needs_refinement(
        camera,
        &tile.bounding_volume,
        tile.geometric_error,
        camera.screen_height,
        camera.sse_threshold,
    );

    tile.needs_refinement_flag = Some(needs_refinement);

    if needs_refinement {
        if let Some(children) = tile.children.as_mut() {
            for child in children.iter_mut() {
                process_tile(source, client, camera, tileset, child)?;
            }
        }
    } else {
        force_refinement(tile, Some(false), true);
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
                    access_key: Some(key.clone()),
                    session: None,
                    loaded: None,
                    key: hash_uri(url),
                });
            }
            _ => {
                return Err(AbwError::TileLoading("Unsupported source type".into()));
            }
        }
    }

    let mut tile = root.as_mut().unwrap();
    build_child_tile_content(&None, tile);
    let _ = process_tile_content(source, client, camera, &None, &mut tile)?;
    Ok(())
}

pub fn send_load_tile(
    tile_src: &Pri<'_>,
    pager_tx: &mut Sender<TilePipelineMessage>,
    gen: Gen,
) -> Result<(), AbwError> {
    let tile = TileContent {
        uri: tile_src.tile_content.uri.clone(),
        state: crate::content::types::TileState::ToLoad,
    };
    pager_tx.try_send(TilePipelineMessage::Load((
        TileMessage {
            key: tile_src.tile_content.key,
            gen: gen,
        },
        tile,
    )))
}

pub fn send_unload_tile(
    id: TileKey,
    pager_tx: &mut Sender<TilePipelineMessage>,
    gen: Gen,
) -> Result<(), AbwError> {
    pager_tx.try_send(TilePipelineMessage::Unload(TileMessage {
        key: id,
        gen: gen,
    }))
}

pub fn send_update_tile(
    tile_src: &Pri<'_>,
    pager_tx: &mut Sender<TilePipelineMessage>,
    gen: Gen,
) -> Result<(), AbwError> {
    if let Some(tile_info) = &tile_src.tile_info {
        return pager_tx.try_send(TilePipelineMessage::Update((
            TileMessage {
                key: tile_src.tile_content.key,
                gen: gen,
            },
            tile_info.clone(),
        )));
    }
    Err(AbwError::TileLoading("No tile info to update".into()))
}

pub fn parser_iteration(
    source: &Source,
    client: &Client,
    camera_data: &CameraRefinementData,
    root: &mut Option<TileSourceContent>,
    pipeline_state: &TileManager,
    pager_tx: &mut Sender<TilePipelineMessage>,
    gen: Gen,
) -> Result<(), AbwError> {
    go(source, client, camera_data, root)?;

    if let Some(tile) = root {
        if let Some(TileSourceContentState::LoadedTileSet { permanent, .. }) = &tile.loaded {
            if let Some(permanent_root) = permanent.as_ref() {
                if let Some(root) = &permanent_root.root {
                    // gather priority tiles
                    let mut priority_list: Vec<Pri> = Vec::new();

                    priortize(pipeline_state, camera_data, root, &mut priority_list)?;

                    // send as many as we can into the pipeline
                    for pri in priority_list.iter() {
                        if !pipeline_state.is_tile_loaded(pri.tile_content.key) {
                            if let Err(_err) = send_load_tile(pri, pager_tx, gen) {
                                // the channel is full, we will try again next time
                                break;
                            }

                            pipeline_state.mark_tile_loaded(pri.tile_content.key);
                        } else {
                            if let Some(tile_info) = &pri.tile_info {
                                if !pipeline_state
                                    .compare_tile_info(pri.tile_content.key, tile_info)
                                {
                                    if let Err(_err) = send_update_tile(pri, pager_tx, gen) {
                                        // the channel is full, we will try again next time
                                        break;
                                    }

                                    pipeline_state.add_or_update_tile_info(
                                        pri.tile_content.key,
                                        tile_info.clone(),
                                    );
                                }
                            }
                        }
                    }

                    /*                     // need a list of tiles currently in the pipeline state, but not in the priority list
                    // these can be removed
                    let mut to_remove: Vec<u64> = Vec::new();
                    for tile_id in pipeline_state.iter() {
                        if !priority_list
                            .iter()
                            .any(|pri| pri.tile_content.id == *tile_id.0)
                        {
                            if let Err(_err) = send_unload_tile(*tile_id.0, pager_tx) {
                                // the channel is full, we will try again next time
                                break;
                            }

                            to_remove.push(*tile_id.0);
                        }
                    }
                    // remove the tiles from the pipeline state
                    pipeline_state.retain(|id, _count| !to_remove.contains(id)); */
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
    let pipeline_state = TileManager::new();

    let mut last_cam_gen = 0;
    let mut gen = 1;
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
                &pipeline_state,
                pager_tx,
                gen,
            )?;

            //last_cam_gen = new_gen;
            gen += 1;
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
