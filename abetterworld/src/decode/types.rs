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
pub struct DecodedMesh {
    pub vertices: *mut Vertex,
    pub vertex_count: i32,
    pub indices: *mut u32,
    pub index_count: i32,
}

pub struct OwnedDecodedMesh {
    pub(crate) inner: DecodedMesh,
    pub material_index: Option<usize>,
}

impl OwnedDecodedMesh {
    pub fn as_vertex_slice(&self) -> &[Vertex] {
        unsafe { std::slice::from_raw_parts(self.inner.vertices, self.inner.vertex_count as usize) }
    }
    pub fn as_index_slice(&self) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self.inner.indices, self.inner.index_count as usize) }
    }
}
