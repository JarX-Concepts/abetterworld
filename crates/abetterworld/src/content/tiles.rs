// ─── Crate: content ────────────────────────────────────────────────────────────
use crate::content::{
    build_materials, build_meshes, build_nodes, download_content, parse_glb,
    parse_textures_from_gltf, upload_textures_to_gpu, Client, TileContent, TileMessage,
    TilePipelineMessage,
};

// ─── Crate: content::types ─────────────────────────────────────────────────────
use crate::content::types::TileState;

use crate::decode::DracoClient;
use crate::helpers::channel::{Receiver, Sender};
// ─── Crate: helpers ────────────────────────────────────────────────────────────
use crate::helpers::{AbwError, TileLoadingContext};
use crate::render::{Mesh, RenderableState};

// ─── External ──────────────────────────────────────────────────────────────────
use bytes::Bytes;
use std::sync::Arc;
use wgpu::util::DeviceExt;

fn download_content_for_tile_shared(
    load: &TileContent,
    content_type: String,
    bytes: Bytes,
) -> Result<Vec<u8>, AbwError> {
    if content_type != "model/gltf-binary" {
        log::error!(
            "Unsupported content type: URI: {}, Content-Type: {}, Bytes: {:?}",
            load.uri,
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
    load: &TileContent,
) -> Result<Vec<u8>, AbwError> {
    let (content_type, bytes) = download_content(&client, &load.uri).await?;
    download_content_for_tile_shared(load, content_type, bytes)
}

async fn process_content_bytes(
    decoder: Arc<DracoClient>,
    load: &mut TileContent,
    bytes: Vec<u8>,
) -> Result<(), AbwError> {
    let _span = tracing::debug_span!("process_content_bytes",).entered();

    let (gltf_json, gltf_bin) =
        parse_glb(&bytes).tile_loading(&format!("Failed to parse GLB: URI: {}", load.uri,))?;

    let meshes = build_meshes(decoder, &gltf_json, &gltf_bin)
        .await
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
    header: TileMessage,
    mut tile: TileContent,
    render_time: &mut Sender<TilePipelineMessage>,
    decoder: Arc<DracoClient>,
) -> Result<(), AbwError> {
    if tile.state == TileState::ToLoad {
        if let Err(e) = content_load(&client, &mut tile, decoder).await {
            log::error!("load failed: {e}");
            return Err(e);
        }

        // that's a bit hacky, but we want to avoid cloning the tile
        let _ = render_time
            .send(TilePipelineMessage::Load((header, tile)))
            .await;
    }
    Ok(())
}

pub async fn wait_and_load_content(
    client: &Client,
    rx: &mut Receiver<TilePipelineMessage>,
    render_time: &mut Sender<TilePipelineMessage>,
) -> Result<(), AbwError> {
    let decoder = Arc::new(DracoClient::new());
    while let Ok(tile) = rx.recv().await {
        match tile {
            TilePipelineMessage::Unload(id) => {
                let _ = render_time.send(TilePipelineMessage::Unload(id)).await;
                continue;
            }
            TilePipelineMessage::Load((h, t)) => {
                load_content(client, h, t, render_time, decoder.clone()).await?;
            }
            TilePipelineMessage::Update(message) => {
                let _ = render_time.send(TilePipelineMessage::Update(message)).await;
                continue;
            }
        }
    }
    Ok(())
}

pub async fn content_load(
    client: &Client,
    tile: &mut TileContent,
    decoder: Arc<DracoClient>,
) -> Result<(), AbwError> {
    if tile.state != TileState::ToLoad {
        return Err(AbwError::TileLoading(format!(
            "Tile is not in ToLoad state: {}",
            tile.uri
        )));
    }

    let data = download_content_for_tile(client, &tile).await?;
    process_content_bytes(decoder, tile, data).await
}

pub fn content_render_setup(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture_bind_group_layout: &wgpu::BindGroupLayout,
    tile: TileContent, // you own this
) -> Result<RenderableState, AbwError> {
    // Consume tile.state and move its contents out
    let TileState::Decoded {
        nodes,
        meshes,
        textures,
        materials,
    } = tile.state
    else {
        return Err(AbwError::TileLoading("Tile is not in Decoded state".into()));
    };

    // You still have the moved-out values by ownership now
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
        nodes, // moved, not cloned
        meshes: return_meshes,
        textures: textures.into_iter().map(|t| t.into()).collect(),
        materials, // moved, not cloned
    })
}
