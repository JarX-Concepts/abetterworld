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
use crate::TILESET_CACHE;
use bytes::Bytes;
use cgmath::Vector3;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::error::Error;
use wgpu::util::DeviceExt;

const CESIUM_ION_ACCESS_TOKEN: &str = "";
const CESIUM_ION_ASSET_ID: u32 = 5;

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

#[derive(Debug, Deserialize)]
pub struct BoundingVolume {
    #[serde(rename = "box")]
    bounding_box: [f64; 12],
}

pub struct OrientedBoundingBox {
    pub center: Vector3<f64>,
    pub half_axes: [Vector3<f64>; 3], // U, V, W
}

impl BoundingVolume {
    /// Converts the bounding volume from Z-up to Y-up, and applies a position offset.
    pub fn to_obb_y_up_with_offset(&self, offset: Vector3<f64>) -> OrientedBoundingBox {
        let b = &self.bounding_box;
        // Center with Y-up and apply offset
        let center = Vector3::new(b[0], b[2], -b[1]) - offset;
        // Half-axes with Y-up (axes don't get offset, just reoriented)
        let half_axes = [
            Vector3::new(b[3], b[5], -b[4]),
            Vector3::new(b[6], b[8], -b[7]),
            Vector3::new(b[9], b[11], -b[10]),
        ];
        OrientedBoundingBox { center, half_axes }
    }
}

#[derive(Debug, Deserialize)]
struct GltfTile {
    #[serde(rename = "boundingVolume")]
    bounding_volume: BoundingVolume,
    #[serde(rename = "geometricError")]
    geometric_error: f64,
    content: Option<GltfTileContent>,
    children: Option<Vec<GltfTile>>,
}

#[derive(Debug, Deserialize)]
struct GltfTileContent {
    uri: String,
}

struct Connection {
    client: Client,
    key: String,
}

struct ConnectionState<'a> {
    connection: &'a Connection,
    tileset_url: &'a String,
    tile: Option<&'a GltfTile>,
    session: Option<String>,
}

fn get_cesium_ion_url(
    asset_id: u32,
    access_token: &str,
) -> Result<CesiumEndpointResponse, Box<dyn Error>> {
    let client = Client::new();
    let request_url = format!("https://api.cesium.com/v1/assets/{}/endpoint", asset_id);

    let response = client.get(&request_url).bearer_auth(access_token).send()?;

    // Print the raw response for debugging
    let raw_text = response.text()?;
    println!("Raw API Response: {}", raw_text);

    let parsed: CesiumApiResponse = serde_json::from_str(&raw_text)?;

    Ok(CesiumEndpointResponse {
        url: parsed.options.url,
    })
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

fn process_tileset(
    camera: &Camera,
    state: &ConnectionState,
) -> Result<Vec<ContentInRange>, Box<dyn Error>> {
    if state.tile.is_none() {
        return Err("No tile found".into());
    }

    let mut tiles = Vec::new();
    let tile_info = state.tile.unwrap();

    let needs_refinement = camera.needs_refinement(
        &tile_info.bounding_volume,
        tile_info.geometric_error,
        1024.0,
        15.0,
    );

    if !needs_refinement || tile_info.children.is_none() {
        if let Some(content) = &tile_info.content {
            let tile_url = resolve_url(&state.tileset_url, &content.uri)?;

            if tile_url.ends_with(".glb") {
                if let Some(session) = &state.session {
                    tiles.push(ContentInRange {
                        uri: tile_url,
                        session: session.clone(),
                    });
                }
            } else {
                // Recurse into the nested tileset
                let mut session_update: String;

                if let Some(session) = &state.session {
                    session_update = session.clone();
                } else {
                    session_update = String::new();
                }

                if let Some((_, new_session)) = tile_url.split_once("session=") {
                    session_update = String::from(new_session);
                }

                return import_tileset(
                    camera,
                    &ConnectionState {
                        connection: state.connection,
                        session: Some(session_update),
                        tileset_url: &tile_url,
                        tile: None,
                    },
                );
            }
        }
    } else {
        if let Some(children) = &tile_info.children {
            for child in children {
                let new_tiles = process_tileset(
                    camera,
                    &ConnectionState {
                        connection: state.connection,
                        tileset_url: state.tileset_url,
                        tile: Some(child),
                        session: state.session.clone(),
                    },
                )?;
                tiles.extend(new_tiles);
            }
        }
    }
    Ok(tiles)
}

fn import_tileset(
    camera: &Camera,
    state: &ConnectionState,
) -> Result<Vec<ContentInRange>, Box<dyn Error>> {
    let url = state.tileset_url;

    // Helper closure to process downloaded or cached content
    let process_content =
        |content_type: &str, bytes: &Bytes| -> Result<Vec<ContentInRange>, Box<dyn Error>> {
            match content_type {
                "application/json" => {
                    let tileset: GltfTileset = serde_json::from_slice(bytes)?;
                    process_tileset(
                        camera,
                        &ConnectionState {
                            connection: state.connection,
                            tileset_url: state.tileset_url,
                            tile: Some(&tileset.root),
                            session: state.session.clone(),
                        },
                    )
                }
                _ => Err(format!(
                    "{}, {}, Unsupported content type: {} - {:?}",
                    state.tileset_url, state.connection.key, content_type, bytes
                )
                .into()),
            }
        };

    let (content_type, bytes) = download_content(
        &state.connection.client,
        state.tileset_url,
        &state.connection.key,
        state.session.as_deref(),
    )?;

    process_content(&content_type, &bytes)
}

fn download_content(
    client: &Client,
    content_url: &str,
    key: &str,
    session: Option<&str>,
) -> Result<(String, Bytes), Box<dyn Error>> {
    // Try cache first
    if let Some((content_type, bytes)) = TILESET_CACHE.get(content_url) {
        return Ok((content_type, bytes));
    }

    println!("Downloading content from: {}", content_url);

    let mut query_params = vec![("key", key)];

    if let Some(session) = session.as_deref() {
        query_params.push(("session", session));
    }

    let response = client.get(content_url).query(&query_params).send()?;

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let bytes = response.bytes()?;

    TILESET_CACHE.insert(content_url.to_string(), content_type.clone(), bytes.clone());

    Ok((content_type, bytes))
}

pub fn load_root() -> Result<(String, String), Box<dyn Error>> {
    let tileset = get_cesium_ion_url(CESIUM_ION_ASSET_ID, CESIUM_ION_ACCESS_TOKEN)?;

    // extract key and session from the url
    let key = tileset.url.split("key=").last();
    let key = match key {
        Some(k) => k,
        None => {
            eprintln!("Error: No key found in the URL");
            return Err("No key found in the URL".into());
        }
    };

    // remove key and session from the url
    let url = tileset.url.replace(&format!("key={}", key), "");

    // remove trailing ?
    let url = url.trim_end_matches('?');

    Ok((url.to_string(), key.to_string()))
}

fn content_load(
    connection: &Connection,
    load: &ContentInRange,
) -> Result<ContentLoaded, Box<dyn Error>> {
    let (content_type, bytes) = download_content(
        &connection.client,
        &load.uri,
        &connection.key,
        Some(&load.session),
    )?;

    if content_type != "model/gltf-binary" {
        return Err(format!(
            "{}, {}, Unsupported content type: {} - {:?}",
            load.uri, connection.key, content_type, bytes
        )
        .into());
    }

    /*     // save to a glb file
    let filename = format!("{}.glb", "testfile.glb");
    std::fs::write(&filename, &bytes);
    println!("Saved GLB to: {}", filename); */

    let gltf = parse_glb(&bytes.to_vec())?;
    let gltf_json = gltf.0;
    let gltf_bin = gltf.1;
    let meshes = build_meshes(&gltf_json, &gltf_bin)?;
    let textures = parse_textures_from_gltf(&gltf_json, &gltf_bin)?;
    let materials = build_materials(&gltf_json)?;
    let nodes = build_nodes(&gltf_json)?;

    Ok(ContentLoaded {
        uri: load.uri.clone(),
        nodes,
        meshes,
        textures,
        materials,
    })
}

fn content_render(
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
        nodes: content.nodes.clone(),
        meshes: return_meshes,
        textures: textures,
        materials: content.materials.clone(),
    })
}

pub struct TileContent {
    connection: Connection,
    root_tileset: String,
    latest_in_range: Vec<ContentInRange>,
    latest_loaded: Vec<ContentLoaded>,
    pub latest_render: Vec<ContentRender>,
}

impl TileContent {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let (url, key) = load_root()?;

        Ok(Self {
            connection: Connection {
                client: Client::new(),
                key: key,
            },
            root_tileset: url,
            latest_in_range: Vec::new(),
            latest_loaded: Vec::new(),
            latest_render: Vec::new(),
        })
    }

    pub fn update_in_range(&mut self, camera: &Camera) -> Result<(), Box<dyn Error>> {
        self.latest_in_range = import_tileset(
            camera,
            &ConnectionState {
                connection: &self.connection,
                tileset_url: &self.root_tileset,
                tile: None,
                session: None,
            },
        )?;

        // remove duplicates
        self.latest_in_range.sort_by(|a, b| a.uri.cmp(&b.uri));
        self.latest_in_range.dedup_by(|a, b| a.uri == b.uri);

        Ok(())
    }

    pub fn update_loaded(&mut self) -> Result<(), Box<dyn Error>> {
        for in_range in &self.latest_in_range {
            // Check if the content is already loaded
            if self
                .latest_loaded
                .iter()
                .any(|loaded| loaded.uri == in_range.uri)
            {
                continue;
            }
            let loaded = content_load(&self.connection, in_range)?;
            self.latest_loaded.push(loaded);
        }
        self.latest_loaded.retain(|loaded| {
            self.latest_in_range
                .iter()
                .any(|in_range| in_range.uri == loaded.uri)
        });

        Ok(())
    }

    pub fn update_render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Result<(), Box<dyn Error>> {
        for loaded in &self.latest_loaded {
            // Check if the content is already loaded
            if self
                .latest_render
                .iter()
                .any(|render| render.uri == loaded.uri)
            {
                continue;
            }
            let render = content_render(device, queue, texture_bind_group_layout, loaded)?;
            self.latest_render.push(render);
        }
        self.latest_render.retain(|render| {
            self.latest_loaded
                .iter()
                .any(|loaded| loaded.uri == render.uri)
        });

        Ok(())
    }
}
