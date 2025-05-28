use crate::{camera::Uniforms, ffi::draco_wrapper::OwnedDecodedMesh};

#[derive(Debug, Clone, PartialEq)]
pub struct ContentInRange {
    pub uri: String,
    pub session: String,
}

pub struct ContentLoaded {
    pub uri: String,
    pub nodes: Vec<Node>,
    pub meshes: Vec<OwnedDecodedMesh>,
    pub textures: Vec<Texture>,
    pub materials: Vec<Material>,
}

// SAFETY: ContentLoaded only contains pointers that are managed elsewhere and are safe to send/share between threads.
unsafe impl Send for ContentLoaded {}
unsafe impl Sync for ContentLoaded {}

pub struct ContentRender {
    pub uri: String,
    pub nodes: Vec<Node>,
    pub meshes: Vec<Mesh>,
    pub textures: Vec<TextureResource>,
    pub materials: Vec<Material>,
}

pub struct TextureResource {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub bind_group: wgpu::BindGroup,
}

pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    // add more metadata as needed (mime, index, etc)
}

#[derive(Clone)]
pub struct Node {
    pub matrix: Uniforms,
    pub mesh_indices: Vec<usize>,
}

#[derive(Clone)]
pub struct Material {
    pub base_color_texture_index: Option<usize>,
    // Add more as needed (base_color_factor, metallic_roughness_texture, etc.)
}

pub struct Mesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub num_indices: u32,
    pub material_index: Option<usize>,
}

pub struct Tile {
    glb_name: String,
    nodes: Vec<Node>,
    meshes: Vec<Mesh>,
    textures: Vec<TextureResource>,
    materials: Vec<Material>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DebugVertex {
    pub position: [f32; 3],
}

impl DebugVertex {
    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<DebugVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            }],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub texcoord0: [f32; 2],
    pub texcoord1: [f32; 2],
}

impl Vertex {
    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // Positions
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // Normals
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // Colors
                wgpu::VertexAttribute {
                    offset: (mem::size_of::<[f32; 3]>() * 2) as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // Texcoord0
                wgpu::VertexAttribute {
                    offset: (mem::size_of::<[f32; 3]>() * 2 + mem::size_of::<[f32; 4]>())
                        as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // Texcoord1
                wgpu::VertexAttribute {
                    offset: (mem::size_of::<[f32; 3]>() * 2
                        + mem::size_of::<[f32; 4]>()
                        + mem::size_of::<[f32; 2]>())
                        as wgpu::BufferAddress,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}
