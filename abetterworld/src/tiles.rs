use crate::content::Mesh;
use crate::content::Tile;
use crate::download::download_content;
use crate::errors::AbwError;
use crate::errors::TileLoadingContext;
use crate::importer::build_materials;
use crate::importer::build_meshes;
use crate::importer::build_nodes;
use crate::importer::parse_glb;
use crate::importer::parse_textures_from_gltf;
use crate::importer::upload_textures_to_gpu;
use reqwest::blocking::Client;
use wgpu::util::DeviceExt;

pub fn download_content_for_tile(
    client: &Client,
    key: &str,
    load: &TileToLoad,
) -> Result<Vec<u8>, AbwError> {
    let (content_type, bytes) = download_content(&client, &load.uri, key, load.session.as_deref())?;

    if content_type != "model/gltf-binary" {
        log::error!(
            "Unsupported content type: URI: {}, Key: {}, Session: {}, Content-Type: {}, Bytes: {:?}",
            load.uri,
            key,
            load.session.as_deref().unwrap_or("None"),
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

pub fn process_content_bytes(load: &Tile, bytes: Vec<u8>) -> Result<ContentLoaded, AbwError> {
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

    Ok(ContentLoaded {
        uri: load.uri.to_string(),
        volume: load.volume.clone(),
        nodes,
        meshes,
        textures,
        materials,
    })
}

pub fn content_load(client: &Client, key: &str, tile: &Tile) -> Result<ContentLoaded, AbwError> {
    let data = download_content_for_tile(client, key, &tile)?;
    process_content_bytes(&tile, data)
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
