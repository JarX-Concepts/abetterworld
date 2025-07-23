use crate::cache::get_tileset_cache;
use crate::content::ContentInRange;
use crate::content::ContentLoaded;
use crate::content::ContentRender;
use crate::content::Mesh;
use crate::importer::build_materials;
use crate::importer::build_meshes;
use crate::importer::build_nodes;
use crate::importer::parse_glb;
use crate::importer::parse_textures_from_gltf;
use crate::importer::upload_textures_to_gpu;
use crate::Camera;
use async_recursion::async_recursion;
use bytes::Bytes;
use cgmath::Matrix3;
use cgmath::SquareMatrix;
use cgmath::Vector3;
use cgmath::Zero;
use reqwest::Client;
use serde::Deserialize;
use std::error::Error;
use std::sync::Arc;
use url::Url;
use wgpu::util::DeviceExt;

#[cfg(not(target_arch = "wasm32"))]
pub trait SendSyncBounds: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> SendSyncBounds for T {}

#[cfg(target_arch = "wasm32")]
pub trait SendSyncBounds {}
#[cfg(target_arch = "wasm32")]
impl<T> SendSyncBounds for T {}

const CESIUM_ION_ACCESS_TOKEN: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJqdGkiOiJkMzMxNGZlYi1iYzcxLTQzMjItOGU0Mi0yYjA3Y2ZmMDRiNWMiLCJpZCI6MTI2NTQwLCJpYXQiOjE2Nzc1MzYyOTl9.2S8ESSboEWY4nxbdGJ9vMgdh9pO2pz42L-PV4KwUlK0";
const CESIUM_ION_ASSET_ID: u32 = 2275207; // Replace with your asset ID

const GOOGLE_API_KEY: &str = "AIzaSyDrSNqujmAmhhZtenz6MEofEuITd3z0JM0";
const GOOGLE_API_URL: &str = "https://tile.googleapis.com/v1/3dtiles/root.json";

#[derive(Debug, Deserialize)]
struct CesiumEndpointResponse {
    url: String,
}

#[derive(Deserialize, Debug)]
struct CesiumApiResponse {
    options: CesiumOptions,
    // Ignore other fields you don't care about
}

#[derive(Deserialize, Debug)]
struct CesiumOptions {
    url: String,
}

#[derive(Debug, Deserialize)]
struct GltfTileset {
    root: GltfTile,
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

#[derive(Debug, Deserialize, Clone)]
pub struct GltfTileContent {
    uri: String,
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub client: Arc<Client>,
    pub key: String,
}

#[derive(Debug, Clone)]
pub struct ConnectionState {
    pub connection: Connection,
    pub tileset_url: String,
    pub tile: Option<GltfTile>,
    pub session: Option<String>,
}

async fn get_cesium_ion_url(
    client: &Client,
    asset_id: u32,
    access_token: &str,
) -> Result<CesiumEndpointResponse, Box<dyn Error>> {
    /*     let request_url = format!("https://api.cesium.com/v1/assets/{}/endpoint", asset_id);

       let response = client
           .get(&request_url)
           .bearer_auth(access_token)
           .send()
           .await?;

       // Print the raw response for debugging
       let raw_text = response.text().await?;

       let parsed: CesiumApiResponse = serde_json::from_str(&raw_text)?;
    */
    // combine GOOGLE_API_URL with the GOOGLE_API_KEY
    let tile_url = format!("{}?key={}", GOOGLE_API_URL, GOOGLE_API_KEY);

    Ok(CesiumEndpointResponse { url: tile_url })
}

fn resolve_url(base: &str, relative: &str) -> Result<String, Box<dyn Error>> {
    use url::Url;
    let base_url = Url::parse(base)?;
    let mut cleaned = base_url.clone();
    cleaned.set_query(None);
    Ok(if relative.starts_with("http") {
        relative.to_string()
    } else {
        cleaned.join(relative)?.to_string()
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

fn extract_session(url: &str) -> Option<String> {
    url.split_once("session=")
        .map(|(_, session)| session.to_string())
}

#[async_recursion(?Send)]
pub async fn process_tileset_old<F>(
    camera: &Camera,
    state: &ConnectionState,
    on_tile: Arc<F>,
) -> Result<Vec<ContentInRange>, Box<dyn Error>>
where
    F: Fn(&ContentInRange) + SendSyncBounds + 'static,
{
    let mut added_geom = false;
    let Some(tile_info) = &state.tile else {
        return Ok(vec![]);
    };

    let needs_refinement = camera.needs_refinement(
        &tile_info.bounding_volume,
        tile_info.geometric_error,
        1024.0,
        100.0,
    );

    let mut tiles = Vec::new();

    if let Some(content) = &tile_info.content {
        let tile_url = resolve_url(&state.tileset_url, &content.uri)?;
        let refine_mode = tile_info.refine.as_deref().unwrap_or("REPLACE");
        let session = extract_session(&tile_url).or_else(|| state.session.clone());

        if is_nested_tileset(&tile_url) {
            let nested_state = ConnectionState {
                connection: state.connection.clone(),
                session: session,
                tileset_url: tile_url.clone(),
                tile: None,
            };

            let nested = import_tileset(camera, &nested_state, Arc::clone(&on_tile)).await?;
            tiles.extend(nested);
        } else if is_glb(&tile_url)
            && (refine_mode == "ADD" || tile_info.children.is_none() || !needs_refinement)
        {
            //log::info!("Loading tile {}", tile_url);
            added_geom = true;
            if let Some(session) = &state.session {
                let result = ContentInRange {
                    uri: tile_url,
                    session: session.clone(),
                    volume: tile_info.bounding_volume.clone(),
                };
                on_tile(&result);
                tiles.push(result);
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
                let child_state = ConnectionState {
                    connection: state.connection.clone(),
                    tileset_url: state.tileset_url.clone(),
                    tile: Some(child.clone()),
                    session: state.session.clone(),
                };
                let child_tiles =
                    process_tileset(camera, &child_state, Arc::clone(&on_tile)).await?;
                tiles.extend(child_tiles);
            }
        }
    }

    Ok(tiles)
}

pub async fn import_tileset_old<F>(
    camera: &Camera,
    state: &ConnectionState,
    on_tile: Arc<F>,
) -> Result<Vec<ContentInRange>, Box<dyn Error>>
where
    F: Fn(&ContentInRange) + SendSyncBounds + 'static,
{
    let (content_type, bytes) = download_content(
        &state.connection.client,
        state.tileset_url.as_str(),
        state.connection.key.as_str(),
        state.session.as_deref(),
    )
    .await?;

    match content_type.as_str() {
        "application/json" | "application/json; charset=UTF-8" => {
            let tileset: GltfTileset = serde_json::from_slice(&bytes)?;
            let state = ConnectionState {
                connection: state.connection.clone(),
                tileset_url: state.tileset_url.clone(),
                tile: Some(tileset.root),
                session: state.session.clone(),
            };
            process_tileset(camera, &state, on_tile).await
        }
        _ => Err(format!(
            "Unsupported content type: {} for {} ({})",
            content_type, state.tileset_url, state.connection.key
        )
        .into()),
    }
}
async fn download_content(
    client: &Client,
    content_url: &str,
    key: &str,
    session: Option<&str>,
) -> Result<(String, Bytes), Box<dyn Error>> {
    // Try cache first
    if let Some(cache) = get_tileset_cache() {
        if let Some((content_type, bytes)) = cache.get(content_url).await {
            return Ok((content_type, bytes));
        }
    }

    //log::info!("Downloading content from: {}", content_url);

    let mut query_params = vec![("key", key)];

    if let Some(session) = session.as_deref() {
        query_params.push(("session", session));
    }

    let response = client.get(content_url).query(&query_params).send().await?;

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let expected_len = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok());

    let bytes = response.bytes().await?;

    if let Some(expected) = expected_len {
        if bytes.len() < expected {
            log::error!(
                "Truncated content: expected {} bytes, got {}",
                expected,
                bytes.len()
            );
        }
    }

    if let Some(cache) = get_tileset_cache() {
        cache
            .insert(content_url.to_string(), content_type.clone(), bytes.clone())
            .await;
    }

    Ok((content_type, bytes))
}

pub async fn load_root(client: &Client) -> Result<(String, String), Box<dyn Error>> {
    let tileset = get_cesium_ion_url(client, CESIUM_ION_ASSET_ID, CESIUM_ION_ACCESS_TOKEN).await;

    let tileset = match tileset {
        Ok(ts) => ts,
        Err(e) => {
            log::error!("Error fetching Cesium Ion URL: {}", e);
            return Err(e.into());
        }
    };

    // extract key and session from the url
    let key = tileset.url.split("key=").last();
    let key = match key {
        Some(k) => k,
        None => {
            log::error!("Error: No key found in the URL");
            return Err("No key found in the URL".into());
        }
    };

    // remove key and session from the url
    let url = tileset.url.replace(&format!("key={}", key), "");

    // remove trailing ?
    let url = url.trim_end_matches('?');

    Ok((url.to_string(), key.to_string()))
}

pub async fn download_content_for_tile(
    client: &Client,
    key: &str,
    load: &ContentInRange,
) -> Result<(Vec<u8>, String), Box<dyn Error>> {
    let (content_type, bytes) = download_content(client, &load.uri, key, Some(&load.session))
        .await
        .map_err(|e| {
            log::error!(
                "Failed to download content: URI: {}, Key: {}, Session: {}, Error: {}",
                load.uri,
                key,
                load.session,
                e
            );
            e
        })?;

    if content_type != "model/gltf-binary" {
        log::error!(
            "Unsupported content type: URI: {}, Key: {}, Session: {}, Content-Type: {}, Bytes: {:?}",
            load.uri,
            key,
            load.session,
            content_type,
            bytes
        );
        return Err(format!(
            "Unsupported content type: URI: {}, Content-Type: {}, Bytes: {:?}",
            load.uri, content_type, bytes
        )
        .into());
    }

    Ok((bytes.to_vec(), content_type))
}

pub fn process_content_bytes(
    load: &ContentInRange,
    session: &str,
    bytes: Vec<u8>,
) -> Result<ContentLoaded, Box<dyn Error>> {
    let (gltf_json, gltf_bin) = parse_glb(&bytes).map_err(|e| {
        log::error!(
            "Failed to parse GLB: URI: {}, Session: {}, Error: {}",
            load.uri,
            session,
            e
        );
        e
    })?;

    let meshes = build_meshes(&gltf_json, &gltf_bin).map_err(|e| {
        log::error!(
            "Failed to build meshes: URI: {}, Session: {}, Error: {}",
            load.uri,
            session,
            e
        );
        e
    })?;

    let textures = parse_textures_from_gltf(&gltf_json, &gltf_bin).map_err(|e| {
        log::error!(
            "Failed to parse textures: URI: {}, Session: {}, Error: {}",
            load.uri,
            session,
            e
        );
        e
    })?;

    let materials = build_materials(&gltf_json).map_err(|e| {
        log::error!(
            "Failed to build materials: URI: {}, Session: {}, Error: {}",
            load.uri,
            session,
            e
        );
        e
    })?;

    let nodes = build_nodes(&gltf_json).map_err(|e| {
        log::error!(
            "Failed to build nodes: URI: {}, Session: {}, Error: {}",
            load.uri,
            session,
            e
        );
        e
    })?;

    Ok(ContentLoaded {
        uri: load.uri.to_string(),
        volume: load.volume.clone(),
        nodes,
        meshes,
        textures,
        materials,
    })
}

pub async fn content_load(
    client: &Client,
    key: &str,
    load: &ContentInRange,
) -> Result<ContentLoaded, Box<dyn Error>> {
    let (content_type, bytes) =
        match download_content(&client, &load.uri, &key, Some(&load.session)).await {
            Ok(res) => res,
            Err(e) => {
                log::error!(
                    "Failed to download content: URI: {}, Key: {}, Session: {}, Error: {}",
                    load.uri,
                    key,
                    load.session,
                    e
                );
                return Err(e);
            }
        };

    if content_type != "model/gltf-binary" {
        log::error!(
            "Unsupported content type: URI: {}, Key: {}, Session: {}, Content-Type: {}, Bytes: {:?}",
            load.uri,
            key,
            load.session,
            content_type,
            bytes
        );
        return Err(format!(
            "Unsupported content type: URI: {}, Key: {}, Content-Type: {}, Bytes: {:?}",
            load.uri, key, content_type, bytes
        )
        .into());
    }

    let gltf = match parse_glb(&bytes.to_vec()) {
        Ok(parsed) => parsed,
        Err(e) => {
            log::error!(
                "Failed to parse GLB: URI: {}, Key: {}, Session: {}, Error: {}",
                load.uri,
                key,
                load.session,
                e
            );
            return Err(e.into());
        }
    };

    let gltf_json = gltf.0;
    let gltf_bin = gltf.1;

    let meshes = match build_meshes(&gltf_json, &gltf_bin) {
        Ok(m) => m,
        Err(e) => {
            log::error!(
                "Failed to build meshes: URI: {}, Key: {}, Session: {}, Error: {}",
                load.uri,
                key,
                load.session,
                e
            );
            return Err(e.into());
        }
    };

    let textures = match parse_textures_from_gltf(&gltf_json, &gltf_bin) {
        Ok(t) => t,
        Err(e) => {
            log::error!(
                "Failed to parse textures: URI: {}, Key: {}, Session: {}, Error: {}",
                load.uri,
                key,
                load.session,
                e
            );
            return Err(e.into());
        }
    };

    let materials = match build_materials(&gltf_json) {
        Ok(m) => m,
        Err(e) => {
            log::error!(
                "Failed to build materials: URI: {}, Key: {}, Session: {}, Error: {}",
                load.uri,
                key,
                load.session,
                e
            );
            return Err(e.into());
        }
    };

    let nodes = match build_nodes(&gltf_json) {
        Ok(n) => n,
        Err(e) => {
            log::error!(
                "Failed to build nodes: URI: {}, Key: {}, Session: {}, Error: {}",
                load.uri,
                key,
                load.session,
                e
            );
            return Err(e.into());
        }
    };

    Ok(ContentLoaded {
        uri: load.uri.clone(),
        volume: load.volume.clone(),
        nodes,
        meshes,
        textures,
        materials,
    })
}

pub fn content_render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture_bind_group_layout: &wgpu::BindGroupLayout,
    content: &ContentLoaded,
) -> Result<ContentRender, std::io::Error> {
    let textures =
        upload_textures_to_gpu(device, queue, &content.textures, texture_bind_group_layout);

    let mut return_meshes = Vec::new();
    for mesh in &content.meshes {
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(mesh.as_vertex_slice()),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(mesh.as_index_slice()),
            usage: wgpu::BufferUsages::INDEX,
        });
        let num_indices = mesh.as_index_slice().len() as u32;

        return_meshes.push(Mesh {
            vertex_buffer,
            index_buffer,
            num_indices,
            material_index: mesh.material_index,
        });
    }

    Ok(ContentRender {
        uri: content.uri.clone(),
        volume: content.volume.clone(),
        nodes: content.nodes.clone(),
        meshes: return_meshes,
        textures: textures,
        materials: content.materials.clone(),
    })
}
