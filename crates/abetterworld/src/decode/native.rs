use super::types::{DecodedMesh, InnerDecodedMesh, OwnedDecodedMesh};

extern "C" {
    fn decode_draco_mesh_interleaved(data: *const u8, len: usize, out: *mut DecodedMesh) -> bool;
    fn free_decoded_mesh(mesh: *mut DecodedMesh);
}

impl Drop for InnerDecodedMesh {
    fn drop(&mut self) {
        unsafe { free_decoded_mesh(&mut self.data) }
    }
}

pub struct DracoClient {}

impl DracoClient {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn decode(&self, data: &[u8]) -> Result<OwnedDecodedMesh, std::io::Error> {
        unsafe {
            let mut mesh = DecodedMesh {
                vertices: std::ptr::null_mut(),
                vertex_count: 0,
                indices: std::ptr::null_mut(),
                index_count: 0,
                job_id: 0,
            };

            if !decode_draco_mesh_interleaved(data.as_ptr(), data.len(), &mut mesh) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Draco decode failed",
                ));
            }

            Ok(OwnedDecodedMesh {
                inner: std::sync::Arc::new(InnerDecodedMesh { data: mesh }),
                material_index: None,
            })
        }
    }
}
