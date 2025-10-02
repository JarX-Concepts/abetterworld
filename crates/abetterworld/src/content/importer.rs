use crate::{
    content::types::{Material, Node, Texture, TextureResource},
    decode::{self, DracoClient, OwnedDecodedMesh},
};
use byteorder::{LittleEndian, ReadBytesExt};
use cgmath::{Deg, Matrix4, One, Quaternion, Vector3, Vector4};
use image::GenericImageView;
use log::error;
use serde_json::Value;
use std::io::{Cursor, Read};

pub fn parse_glb(glb: &[u8]) -> Result<(Value, Vec<u8>), Box<std::io::Error>> {
    let total_len = glb.len();
    let mut cursor = Cursor::new(glb);

    // --- GLB Header ---
    let mut magic = [0; 4];
    if let Err(e) = cursor.read_exact(&mut magic) {
        error!("Failed to read magic header: {}", e);
        return Err(e.into());
    }

    if &magic != b"glTF" {
        error!("Invalid GLB magic header: {:?}", magic);
        return Err(
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid magic header").into(),
        );
    }

    let version = cursor.read_u32::<LittleEndian>()?;
    if version != 2 {
        error!("Unsupported GLB version: {}", version);
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Only GLB v2 is supported",
        )
        .into());
    }

    let _length = cursor.read_u32::<LittleEndian>()?;

    // --- JSON Chunk Header ---
    let json_len = cursor.read_u32::<LittleEndian>()?;
    let json_type = cursor.read_u32::<LittleEndian>()?;

    if json_type != 0x4E4F534A {
        error!("Expected JSON chunk, got: 0x{:X}", json_type);
        return Err(
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Expected JSON chunk").into(),
        );
    }

    let required_len = 12 + 8 + json_len as usize + 8; // base header + json chunk + BIN chunk header
    if total_len < required_len {
        error!(
            "GLB buffer too small before JSON read: expected at least {}, got {}",
            required_len, total_len
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "GLB buffer too small before reading JSON",
        )
        .into());
    }

    let mut json_buf = vec![0u8; json_len as usize];
    if let Err(e) = cursor.read_exact(&mut json_buf) {
        error!("Failed to read JSON chunk: {}", e);
        return Err(e.into());
    }

    let json: Value = match serde_json::from_slice(&json_buf) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to parse JSON chunk: {}", e);
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e).into());
        }
    };

    // --- BIN Chunk Header ---
    let bin_len = cursor.read_u32::<LittleEndian>()?;
    let bin_type = cursor.read_u32::<LittleEndian>()?;
    if bin_type != 0x004E4942 {
        error!("Expected BIN chunk, got: 0x{:X}", bin_type);
        return Err(
            std::io::Error::new(std::io::ErrorKind::InvalidData, "Expected BIN chunk").into(),
        );
    }

    let remaining_bytes = total_len.saturating_sub(cursor.position() as usize);
    if remaining_bytes < bin_len as usize {
        error!(
            "GLB buffer too small for BIN chunk: need {}, have {}",
            bin_len, remaining_bytes
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "GLB buffer too small for BIN chunk",
        )
        .into());
    }

    let mut bin_buf = vec![0u8; bin_len as usize];
    if let Err(e) = cursor.read_exact(&mut bin_buf) {
        error!("Failed to read BIN chunk: {}", e);
        return Err(e.into());
    }

    Ok((json, bin_buf))
}

pub async fn build_meshes(
    decode_client: &DracoClient,
    json: &Value,
    bin: &[u8],
) -> Result<Vec<OwnedDecodedMesh>, std::io::Error> {
    let mut results = Vec::new();

    if let Some(meshes) = json.get("meshes").and_then(|v| v.as_array()) {
        for mesh in meshes {
            if let Some(primitives) = mesh.get("primitives").and_then(|v| v.as_array()) {
                for primitive in primitives {
                    if let Some(draco) = primitive
                        .get("extensions")
                        .and_then(|v| v.get("KHR_draco_mesh_compression"))
                    {
                        if let Some(buffer_view_idx) =
                            draco.get("bufferView").and_then(|v| v.as_u64())
                        {
                            if let Some(buffer_view) = json
                                .get("bufferViews")
                                .and_then(|v| v.get(buffer_view_idx as usize))
                            {
                                let offset = buffer_view
                                    .get("byteOffset")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                let length = buffer_view
                                    .get("byteLength")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                let start = offset as usize;
                                let end = start + length as usize;

                                if end <= bin.len() && start < end {
                                    // Use a slice directly, NOT .to_vec()
                                    let mut mesh_data =
                                        decode_client.decode(&bin[start..end]).await?;

                                    if let Some(material_idx) =
                                        primitive.get("material").and_then(|v| v.as_u64())
                                    {
                                        mesh_data.material_index = Some(material_idx as usize);
                                    }

                                    results.push(mesh_data);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(results)
}

pub fn parse_textures_from_gltf(json: &Value, bin: &[u8]) -> Result<Vec<Texture>, std::io::Error> {
    let mut textures = Vec::new();

    if let Some(images) = json.get("images").and_then(|v| v.as_array()) {
        for image in images {
            let buffer_view_idx = image.get("bufferView").and_then(|v| v.as_u64());
            let mime_type = image
                .get("mimeType")
                .and_then(|v| v.as_str())
                .unwrap_or("image/png");
            if let Some(buffer_view_idx) = buffer_view_idx {
                if let Some(buffer_view) = json
                    .get("bufferViews")
                    .and_then(|v| v.get(buffer_view_idx as usize))
                {
                    let offset = buffer_view
                        .get("byteOffset")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let length = buffer_view
                        .get("byteLength")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let start = offset as usize;
                    let end = (offset + length) as usize;

                    if end <= bin.len() && start < end {
                        let img_bytes = &bin[start..end];

                        let dyn_img = match mime_type {
                            "image/png" | "image/jpeg" => image::load_from_memory(img_bytes)
                                .map_err(|e| {
                                    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
                                })?,
                            _ => {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "Unsupported image type",
                                ))
                            }
                        };

                        let rgba = dyn_img.to_rgba8();
                        let (width, height) = dyn_img.dimensions();

                        textures.push(Texture {
                            width,
                            height,
                            rgba: rgba.into_raw(),
                        });
                    }
                }
            }
        }
    }

    Ok(textures)
}

pub fn upload_textures_to_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture_data: &[Texture],
    texture_bind_group_layout: &wgpu::BindGroupLayout,
) -> Vec<TextureResource> {
    texture_data
        .iter()
        .map(|td| {
            let texture_size = wgpu::Extent3d {
                width: td.width,
                height: td.height,
                depth_or_array_layers: 1,
            };
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("GLTF Texture"),
                size: texture_size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &td.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * td.width),
                    rows_per_image: Some(td.height),
                },
                texture_size,
            );
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("GLTF Texture Sampler"),
                ..Default::default()
            });

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
                label: Some("Texture Bind Group"),
            });

            TextureResource {
                texture,
                view,
                sampler,
                bind_group,
            }
        })
        .collect()
}

pub fn build_materials(json: &Value) -> Result<Vec<Material>, std::io::Error> {
    let mut materials = Vec::new();

    if let Some(materials_json) = json.get("materials").and_then(|v| v.as_array()) {
        for mat in materials_json {
            // Default to None if not found
            let mut base_color_texture_index = None;

            // Look for baseColorTexture index
            if let Some(pbr) = mat.get("pbrMetallicRoughness") {
                if let Some(tex) = pbr.get("baseColorTexture") {
                    if let Some(idx) = tex.get("index").and_then(|v| v.as_u64()) {
                        base_color_texture_index = Some(idx as usize);
                    }
                }
            }

            // Add more parsing as needed for other attributes...

            materials.push(Material {
                base_color_texture_index,
            });
        }
    }

    Ok(materials)
}

pub fn build_nodes(json: &Value) -> Result<Vec<Node>, std::io::Error> {
    let mut nodes = Vec::new();
    let y_up_to_z_up = Matrix4::from_angle_x(Deg(90.0));
    let rotate_around_z = Matrix4::from_angle_y(Deg(180.0));

    if let Some(nodes_json) = json.get("nodes").and_then(|v| v.as_array()) {
        for node_json in nodes_json {
            // Check for a 4x4 "matrix" first (16 floats, column major)
            let matrix =
                if let Some(matrix_json) = node_json.get("matrix").and_then(|v| v.as_array()) {
                    if matrix_json.len() == 16 {
                        let mut m = [0.0f64; 16];
                        for (i, v) in matrix_json.iter().enumerate() {
                            m[i] = v.as_f64().unwrap();
                        }
                        Matrix4::from_cols(
                            Vector4::new(m[0], m[1], m[2], m[3]),
                            Vector4::new(m[4], m[5], m[6], m[7]),
                            Vector4::new(m[8], m[9], m[10], m[11]),
                            Vector4::new(m[12], m[13], m[14], m[15]),
                        )
                    } else {
                        Matrix4::one()
                    }
                } else {
                    // Build from translation, rotation, scale (TRS)
                    let translation = node_json
                        .get("translation")
                        .and_then(|t| t.as_array())
                        .map(|arr| {
                            [
                                arr[0].as_f64().unwrap(),
                                arr[1].as_f64().unwrap(),
                                arr[2].as_f64().unwrap(),
                            ]
                        })
                        .unwrap_or([0.0, 0.0, 0.0]);
                    let rotation = node_json
                        .get("rotation")
                        .and_then(|r| r.as_array())
                        .map(|arr| {
                            [
                                arr[0].as_f64().unwrap(),
                                arr[1].as_f64().unwrap(),
                                arr[2].as_f64().unwrap(),
                                arr[3].as_f64().unwrap(),
                            ]
                        })
                        .unwrap_or([0.0, 0.0, 0.0, 1.0]);
                    let scale = node_json
                        .get("scale")
                        .and_then(|s| s.as_array())
                        .map(|arr| {
                            [
                                arr[0].as_f64().unwrap(),
                                arr[1].as_f64().unwrap(),
                                arr[2].as_f64().unwrap(),
                            ]
                        })
                        .unwrap_or([1.0, 1.0, 1.0]);

                    let t = Matrix4::from_translation(Vector3::new(
                        translation[0],
                        translation[1],
                        translation[2],
                    ));
                    let r = Matrix4::from(Quaternion::new(
                        rotation[3],
                        rotation[0],
                        rotation[1],
                        rotation[2],
                    ));
                    let s = Matrix4::from_nonuniform_scale(scale[0], scale[1], scale[2]);
                    t * r * s
                };

            // Support both "mesh" (single) and "meshes" (array) for flexibility:
            let mesh_indices = if let Some(meshes_json) = node_json.get("meshes") {
                meshes_json
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|m| m.as_u64().map(|idx| idx as usize))
                    .collect()
            } else if let Some(mesh_idx) = node_json.get("mesh").and_then(|m| m.as_u64()) {
                vec![mesh_idx as usize]
            } else {
                vec![]
            };

            let matrix = y_up_to_z_up * matrix;
            nodes.push(Node {
                transform: matrix,
                mesh_indices,
            });
        }
    }

    Ok(nodes)
}
