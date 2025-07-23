use std::sync::Arc;

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub texcoord0: [f32; 2],
    pub texcoord1: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedMesh {
    pub vertices: *mut Vertex,
    pub vertex_count: i32,
    pub indices: *mut u32,
    pub index_count: i32,
}
unsafe impl Send for DecodedMesh {}
unsafe impl Sync for DecodedMesh {}

#[derive(Clone, Debug, PartialEq)]
pub struct InnerDecodedMesh {
    pub data: DecodedMesh,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OwnedDecodedMesh {
    pub(crate) inner: Arc<InnerDecodedMesh>,
    pub material_index: Option<usize>,
}

impl OwnedDecodedMesh {
    pub fn as_vertex_slice(&self) -> &[Vertex] {
        unsafe {
            std::slice::from_raw_parts(
                self.inner.data.vertices,
                self.inner.data.vertex_count as usize,
            )
        }
    }
    pub fn as_index_slice(&self) -> &[u32] {
        unsafe {
            std::slice::from_raw_parts(
                self.inner.data.indices,
                self.inner.data.index_count as usize,
            )
        }
    }
}
