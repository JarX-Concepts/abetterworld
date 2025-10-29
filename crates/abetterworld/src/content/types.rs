use crate::{content::BoundingVolume, decode::OwnedDecodedMesh};

use cgmath::Matrix4;
use smallvec::SmallVec;

pub const MAX_RENDERABLE_TILES: u64 = 1024;
pub const MAX_RENDERABLE_NODES: u64 = 1024;
pub const MAX_RENDERABLE_TILES_US: usize = MAX_RENDERABLE_TILES as usize;
pub const MAX_RENDERABLE_NODES_US: usize = MAX_RENDERABLE_NODES as usize;

pub type TileKey = u64;
pub type Gen = u32;
pub type ChildrenKeys = SmallVec<[TileKey; 8]>;

#[derive(Debug, Clone, PartialEq)]
pub enum TilePipelineMessage {
    Load((TileMessage, TileContent)),
    Unload(TileMessage),
    Update((TileMessage, TileInfo)),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TileMessage {
    pub key: TileKey,
    pub gen: Gen,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TileContent {
    pub uri: String,
    pub state: TileState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TileState {
    ToLoad,
    Decoded {
        nodes: Vec<Node>,
        meshes: Vec<OwnedDecodedMesh>,
        textures: Vec<Texture>,
        materials: Vec<Material>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum RefineMode {
    Add,
    Replace,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TileInfo {
    pub children: Option<ChildrenKeys>,
    pub parent: Option<TileKey>,
    pub volume: BoundingVolume,
    pub refine: RefineMode,
    pub geometric_error: f64,
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
