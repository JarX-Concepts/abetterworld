use super::types::{DecodedMesh, OwnedDecodedMesh};

impl Drop for OwnedDecodedMesh {
    fn drop(&mut self) {
        //unsafe { free_decoded_mesh(&mut self.inner) }
    }
}

extern "C" {
    fn decode_draco_mesh_interleaved(data: *const u8, len: usize, out: *mut DecodedMesh) -> bool;
    fn free_decoded_mesh(mesh: *mut DecodedMesh);
}

pub fn init() -> Result<(), std::io::Error> {
    // No-op for native, as we don't need to initialize anything
    Ok(())
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
