use crate::content::{download_content, BoundingVolume, Client, TileKey};
use crate::dynamics::CameraRefinementData;
use crate::helpers::{hash_uri, spawn_detached, AbwError, TileLoadingContext};
use crate::Source;
use cgmath::{InnerSpace, Point3};
use serde::Deserialize;
use std::sync::{Arc, RwLock};
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
