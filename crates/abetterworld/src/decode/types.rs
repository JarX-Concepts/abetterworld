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
    pub vertex_count: u32,
    pub indices: *mut u32,
    pub index_count: u32,
    pub job_id: u32,
}
unsafe impl Send for DecodedMesh {}
unsafe impl Sync for DecodedMesh {}

#[derive(Clone, Debug, PartialEq)]
pub struct InnerDecodedMesh {
    pub data: DecodedMesh,
    /// If true, vertices/indices were allocated by Rust (Vec) and must be
    /// freed with Vec::from_raw_parts in Drop instead of the C FFI free.
    pub rust_owned: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OwnedDecodedMesh {
    pub(crate) inner: Arc<InnerDecodedMesh>,
    pub material_index: Option<usize>,
}

impl OwnedDecodedMesh {
    /// Create an OwnedDecodedMesh from Rust-owned vertex and index data.
    pub fn from_vertices_and_indices(vertices: Vec<Vertex>, indices: Vec<u32>) -> Self {
        let vertex_count = vertices.len() as u32;
        let index_count = indices.len() as u32;

        // into_boxed_slice guarantees len == capacity, making Drop reconstruction safe
        let mut vertices_box = vertices.into_boxed_slice();
        let mut indices_box = indices.into_boxed_slice();

        let vertices_ptr = vertices_box.as_mut_ptr();
        let indices_ptr = indices_box.as_mut_ptr();

        // Prevent deallocation — Drop will reconstruct and free
        std::mem::forget(vertices_box);
        std::mem::forget(indices_box);

        OwnedDecodedMesh {
            inner: Arc::new(InnerDecodedMesh {
                data: DecodedMesh {
                    vertices: vertices_ptr,
                    vertex_count,
                    indices: indices_ptr,
                    index_count,
                    job_id: 0,
                },
                rust_owned: true,
            }),
            material_index: None,
        }
    }

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
