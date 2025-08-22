use crate::{
    content::{BoundingBox, BoundingVolume},
    decode::OwnedDecodedMesh,
};
use cgmath::Matrix4;
use std::mem;

pub const MAX_RENDERABLE_TILES: u64 = 512;
pub const MAX_RENDERABLE_NODES: u64 = 512;
pub const MAX_RENDERABLE_TILES_US: usize = MAX_RENDERABLE_TILES as usize;
pub const MAX_RENDERABLE_NODES_US: usize = MAX_RENDERABLE_NODES as usize;

#[derive(Debug, Clone, PartialEq)]
pub struct Tile {
    pub counter: u64,
    pub parent: Option<u64>,
    pub id: u64,
    pub uri: String,
    pub volume: BoundingVolume,
    pub state: TileState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TileState {
    Invalid,
    Unload,
    ToLoad,
    Decoded {
        nodes: Vec<Node>,
        meshes: Vec<OwnedDecodedMesh>,
        textures: Vec<Texture>,
        materials: Vec<Material>,
    },
    Renderable,
}

impl Tile {
    pub fn default() -> Self {
        Tile {
            counter: 0,
            parent: None,
            id: 0,
            uri: String::new(),
            volume: BoundingVolume::default(),
            state: TileState::Invalid,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderableState {
    pub tile: Tile,
    pub nodes: Vec<Node>,
    pub meshes: Vec<Mesh>, // Mesh contains wgpu::Buffer
    pub textures: Vec<TextureResource>,
    pub materials: Vec<Material>,
    pub unload: bool,
    pub culling_volume: BoundingBox,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextureResource {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub bind_group: wgpu::BindGroup,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    // add more metadata as needed (mime, index, etc)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub transform: Matrix4<f64>,
    pub mesh_indices: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub base_color_texture_index: Option<usize>,
    // Add more as needed (base_color_factor, metallic_roughness_texture, etc.)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub num_indices: u32,
    pub material_index: Option<usize>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DebugVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

impl DebugVertex {
    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<DebugVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // Colors
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
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
