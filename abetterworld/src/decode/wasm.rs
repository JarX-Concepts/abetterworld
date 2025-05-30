#[cfg(target_arch = "wasm32")]
use super::types::{DecodedMesh, OwnedDecodedMesh, Vertex};
#[cfg(target_arch = "wasm32")]
use js_sys::{Float32Array, Reflect, Uint32Array};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "/js/draco-wrapper.js")]
extern "C" {
    #[wasm_bindgen(catch)]
    fn decodeDracoBuffer(data: &[u8]) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch)]
    fn initDraco() -> Result<JsValue, JsValue>;
}

#[cfg(target_arch = "wasm32")]
pub fn init() -> Result<(), std::io::Error> {
    initDraco().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to initialize Draco decoder",
        )
    })?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn decode(data: &[u8]) -> Result<OwnedDecodedMesh, std::io::Error> {
    let js_result = decodeDracoBuffer(data)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "JS decode failed"))?;

    let obj = js_result
        .dyn_into::<js_sys::Object>()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Unexpected JS object"))?;

    let verts_js = Reflect::get(&obj, &"vertices".into())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Missing vertices"))?;
    let verts_array = Float32Array::new(&verts_js);
    let raw_verts = verts_array.to_vec();

    let indices_js = Reflect::get(&obj, &"indices".into())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "Missing indices"))?;
    let indices_array = Uint32Array::new(&indices_js);
    let raw_indices = indices_array.to_vec();

    if raw_verts.len() % 14 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Malformed vertex data",
        ));
    }

    let vertex_count = raw_verts.len() / 14;
    let index_count = raw_indices.len();

    // Allocate boxed heap arrays (manually match the C layout)
    let mut boxed_vertices = Vec::with_capacity(vertex_count);
    for chunk in raw_verts.chunks_exact(14) {
        boxed_vertices.push(Vertex {
            position: [chunk[0], chunk[1], chunk[2]],
            normal: [chunk[3], chunk[4], chunk[5]],
            color: [chunk[6], chunk[7], chunk[8], chunk[9]],
            texcoord0: [chunk[10], chunk[11]],
            texcoord1: [chunk[12], chunk[13]],
        });
    }
    let mut boxed_indices = raw_indices;

    let vertices_ptr = boxed_vertices.as_mut_ptr();
    let indices_ptr = boxed_indices.as_mut_ptr();

    let mesh = DecodedMesh {
        vertices: vertices_ptr,
        vertex_count: vertex_count as i32,
        indices: indices_ptr,
        index_count: index_count as i32,
    };

    // Prevent Rust from freeing the boxed memory
    std::mem::forget(boxed_vertices);
    std::mem::forget(boxed_indices);

    Ok(OwnedDecodedMesh {
        inner: mesh,
        material_index: None,
    })
}
