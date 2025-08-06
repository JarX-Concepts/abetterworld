// ─── Crate: content ────────────────────────────────────────────────────────────
use crate::content::{
    build_materials, build_meshes, build_nodes, download_content, parse_glb,
    parse_textures_from_gltf, upload_textures_to_gpu, Client, GOOGLE_API_KEY,
};

// ─── Crate: content::types ─────────────────────────────────────────────────────
use crate::content::types::{Mesh, RenderableState, Tile, TileState};

use crate::helpers::channel::{Receiver, Sender};
// ─── Crate: helpers ────────────────────────────────────────────────────────────
use crate::helpers::{AbwError, TileLoadingContext};

// ─── External ──────────────────────────────────────────────────────────────────
use bytes::Bytes;
use std::mem;
use wgpu::util::DeviceExt;

fn download_content_for_tile_shared(
    key: &str,
    load: &Tile,
    content_type: String,
    bytes: Bytes,
) -> Result<Vec<u8>, AbwError> {
    if content_type != "model/gltf-binary" {
        log::error!(
            "Unsupported content type: URI: {}, Key: {}, Content-Type: {}, Bytes: {:?}",
            load.uri,
            key,
            content_type,
            bytes
        );
        return Err(AbwError::TileLoading(format!(
            "Unsupported content type: URI: {}, Content-Type: {}, Bytes: {:?}",
            load.uri, content_type, bytes
        )));
    }

    Ok(bytes.to_vec())
}

async fn download_content_for_tile(
    client: &Client,
    key: &str,
    load: &Tile,
) -> Result<Vec<u8>, AbwError> {
    let (content_type, bytes) = download_content(&client, &load.uri, key, &None).await?;

    download_content_for_tile_shared(key, load, content_type, bytes)
}

fn process_content_bytes(load: &mut Tile, bytes: Vec<u8>) -> Result<(), AbwError> {
    let (gltf_json, gltf_bin) =
        parse_glb(&bytes).tile_loading(&format!("Failed to parse GLB: URI: {}", load.uri,))?;

    let meshes = build_meshes(&gltf_json, &gltf_bin)
        .tile_loading(&format!("Failed to parse GLB meshes: URI: {}", load.uri,))?;

    let textures = parse_textures_from_gltf(&gltf_json, &gltf_bin)
        .tile_loading(&format!("Failed to parse GLB textures: URI: {}", load.uri,))?;

    let materials = build_materials(&gltf_json)
        .tile_loading(&format!("Failed to parse GLB materials: URI: {}", load.uri,))?;

    let nodes = build_nodes(&gltf_json)
        .tile_loading(&format!("Failed to parse GLB nodes: URI: {}", load.uri,))?;

    load.state = TileState::Decoded {
        nodes,
        meshes,
        textures,
        materials,
    };

    Ok(())
}

pub async fn load_content(
    client: &Client,
    tile: &mut Tile,
    render_time: &mut Sender<Tile>,
) -> Result<(), AbwError> {
    if tile.state == TileState::ToLoad {
        if let Err(e) = content_load(&client, GOOGLE_API_KEY, tile).await {
            log::error!("load failed: {e}");
            return Err(e);
        }

        if matches!(tile.state, TileState::Decoded { .. }) {
            // that's a bit hacky, but we want to avoid cloning the tile
            let _ = render_time.send(mem::replace(tile, Tile::default())).await;
        }
    }
    Ok(())
}

pub async fn wait_and_load_content(
    client: &Client,
    rx: &mut Receiver<Tile>,
    render_time: &mut Sender<Tile>,
) -> Result<(), AbwError> {
    while let Ok(mut tile) = rx.recv().await {
        load_content(client, &mut tile, render_time).await?;
    }
    Ok(())
}

pub async fn content_load(client: &Client, key: &str, tile: &mut Tile) -> Result<(), AbwError> {
    if tile.state != TileState::ToLoad {
        return Err(AbwError::TileLoading(format!(
            "Tile is not in ToLoad state: {}",
            tile.uri
        )));
    }

    let data = download_content_for_tile(client, key, &tile).await?;
    process_content_bytes(tile, data)
}

pub fn content_render_setup(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture_bind_group_layout: &wgpu::BindGroupLayout,
    tile: &mut Tile,
) -> Result<RenderableState, AbwError> {
    // hacky way to get ownership of decoded tile state
    let decoded = match mem::replace(&mut tile.state, TileState::Renderable) {
        TileState::Decoded {
            nodes,
            meshes,
            textures,
            materials,
        } => (nodes, meshes, textures, materials),
        other => {
            tile.state = other; // restore original state
            return Err(AbwError::TileLoading(
                "Tile is not in Decoded state".to_owned(),
            ));
        }
    };

    let (nodes, meshes, textures, materials) = decoded;

    let textures = upload_textures_to_gpu(device, queue, &textures, texture_bind_group_layout);

    let return_meshes = meshes
        .into_iter()
        .map(|mesh| {
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

            Mesh {
                vertex_buffer,
                index_buffer,
                num_indices,
                material_index: mesh.material_index,
            }
        })
        .collect();

    Ok(RenderableState {
        nodes,
        meshes: return_meshes,
        textures: textures.into_iter().map(|t| t.into()).collect(),
        materials,
        unload: false,
        culling_volume: tile.volume.to_aabb(),
        tile: tile.to_owned(),
    })
}
