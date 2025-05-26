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
    inner: DecodedMesh,
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

impl Drop for OwnedDecodedMesh {
    fn drop(&mut self) {
        unsafe { free_decoded_mesh(&mut self.inner) }
    }
}

extern "C" {
    fn decode_draco_mesh_interleaved(data: *const u8, len: usize, out: *mut DecodedMesh) -> bool;
    fn free_decoded_mesh(mesh: *mut DecodedMesh);
}

pub fn decode(data: &[u8]) -> Result<OwnedDecodedMesh, std::io::Error> {
    unsafe {
        let mut mesh = DecodedMesh {
            vertices: std::ptr::null_mut(),
            vertex_count: 0,
            indices: std::ptr::null_mut(),
            index_count: 0,
        };

        if !decode_draco_mesh_interleaved(data.as_ptr(), data.len(), &mut mesh) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Draco decode failed",
            ));
        }

        Ok(OwnedDecodedMesh {
            inner: mesh,
            material_index: None,
        })
    }
}
