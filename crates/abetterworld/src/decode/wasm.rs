// src/draco_client.rs
use js_sys::{ArrayBuffer, Float32Array, Object, Promise, Reflect, Uint32Array, Uint8Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::decode::{types::Vertex, DecodedMesh, OwnedDecodedMesh};

#[wasm_bindgen(module = "/js/dist/draco-client.js")]
extern "C" {
    #[wasm_bindgen(js_name = DracoWorkerClient)]
    type JsDracoWorkerClient;

    #[wasm_bindgen(constructor, js_class = "DracoWorkerClient")]
    fn new(worker_url: &str) -> JsDracoWorkerClient;

    /// Returns a small handle object: { jobId, promise, cancel }
    #[wasm_bindgen(method, js_name = decode)]
    fn js_decode(this: &JsDracoWorkerClient, buffer: ArrayBuffer) -> JsValue;

    #[wasm_bindgen(method)]
    fn dispose(this: &JsDracoWorkerClient);
}

// Helper bindings for the decode handle
#[wasm_bindgen]
extern "C" {
    type DecodeHandle;

    #[wasm_bindgen(method, getter, js_name = jobId)]
    fn job_id(this: &DecodeHandle) -> u32;

    #[wasm_bindgen(method, getter)]
    fn promise(this: &DecodeHandle) -> Promise;

    #[wasm_bindgen(method)]
    fn cancel(this: &DecodeHandle);
}

pub struct DracoClient {
    inner: JsDracoWorkerClient,
}

impl DracoClient {
    pub fn new(worker_url: &str) -> Self {
        Self {
            inner: JsDracoWorkerClient::new(worker_url),
        }
    }

    /// Kick off a decode. Returns (future, cancel_closure).
    ///
    /// - `compressed` is the Draco bitstream (e.g., from fetch/IDB).
    /// - `prefer_u16` asks worker to return u16 indices when possible.
    pub fn decode_with_cancel(
        &self,
        compressed: &[u8],
    ) -> (
        impl std::future::Future<Output = Result<DecodedMesh, JsValue>>,
        impl FnOnce(),
    ) {
        // Copy compressed bytes into a transferable ArrayBuffer
        let ab = ArrayBuffer::new(compressed.len() as u32);
        let view = Uint8Array::new(&ab);
        view.copy_from(compressed);

        // Call JS client: get a handle { jobId, promise, cancel }
        let handle_val = self.inner.js_decode(ab);

        let handle_for_fut: DecodeHandle = handle_val.clone().unchecked_into();
        let handle_for_cancel: DecodeHandle = handle_val.unchecked_into();

        let job_id = handle_for_fut.job_id();

        // Build a cancel closure that invokes handle.cancel()
        let canceled = std::rc::Rc::new(std::cell::Cell::new(false));
        let canceled_clone = canceled.clone();
        let cancel_fn = move || {
            if !canceled_clone.get() {
                canceled_clone.set(true);
                handle_for_cancel.cancel();
            }
        };

        // Convert Promise -> Future
        let fut = async move {
            let js = JsFuture::from(handle_for_fut.promise()).await?;

            // js is an object { vertices: Float32Array, indices: Uint32Array, numPoints, numFaces }
            let obj: &Object = js.unchecked_ref();

            /*             let obj = js.dyn_into::<js_sys::Object>().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::Other, "Unexpected JS object")
            })?; */

            let verts_js = Reflect::get(&obj, &"vertices".into())
                .map_err(|_| JsValue::from_str("Missing vertices"))?;
            let verts_array = Float32Array::new(&verts_js);
            let raw_verts = verts_array.to_vec();

            let indices_js = Reflect::get(&obj, &"indices".into())
                .map_err(|_| JsValue::from_str("Missing indices"))?;
            let indices_array = Uint32Array::new(&indices_js);
            let raw_indices = indices_array.to_vec();

            if raw_verts.len() % 14 != 0 {
                return Err(JsValue::from_str("Malformed vertex data"));
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
                vertex_count: vertex_count as u32,
                indices: indices_ptr,
                index_count: index_count as u32,
                job_id,
            };

            // Prevent Rust from freeing the boxed memory
            std::mem::forget(boxed_vertices);
            std::mem::forget(boxed_indices);

            Ok(mesh)
        };

        (fut, cancel_fn)
    }

    pub async fn decode(&self, data: &[u8]) -> Result<OwnedDecodedMesh, std::io::Error> {
        log::info!("WASM decode of {} bytes", data.len());
        // Borrow the client briefly from the thread-local storage to start the decode.
        // The returned future does not hold a long-lived borrow of the client, so this
        // short borrow is safe.
        let (fut, _cancel) = self.decode_with_cancel(data);

        match fut.await {
            Ok(mesh) => {
                log::info!(
                    "WASM decode complete: {} vertices, {} indices",
                    mesh.vertex_count,
                    mesh.index_count
                );

                Ok(OwnedDecodedMesh {
                    inner: std::sync::Arc::new(crate::decode::types::InnerDecodedMesh {
                        data: mesh,
                    }),
                    material_index: None,
                })
            }
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Draco decode failed: {:?}", e),
            )),
        }
    }
}

impl Drop for DracoClient {
    fn drop(&mut self) {
        self.inner.dispose();
    }
}
