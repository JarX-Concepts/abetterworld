// async_global.rs
use core::future::Future;

#[cfg(target_arch = "wasm32")]
#[inline]
pub fn spawn_detached<F>(fut: F)
where
    F: Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(fut);
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use futures::future;
    use once_cell::sync::Lazy;
    use tokio::runtime::{Builder, Handle};

    // One parked runtime for the whole process; we only expose a Handle.
    pub static HANDLE: Lazy<Handle> = Lazy::new(|| {
        let rt = Builder::new_multi_thread()
            .worker_threads(2)
            .enable_io()
            .enable_time()
            .build()
            .expect("tokio runtime");

        let handle = rt.handle().clone();
        // Park the owning runtime forever so it never drops (no shutdown panic).
        std::thread::spawn(move || rt.block_on(future::pending::<()>()));
        handle
    });

    #[inline]
    pub fn spawn_detached<F>(fut: F)
    where
        F: core::future::Future<Output = ()> + Send + 'static,
    {
        HANDLE.spawn(fut);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub fn spawn_detached<F>(fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    native::spawn_detached(fut);
}

// expose the guard so the caller holds it for the needed scope
#[cfg(not(target_arch = "wasm32"))]
pub fn enter_runtime() -> tokio::runtime::EnterGuard<'static> {
    native::HANDLE.enter()
}

#[cfg(target_arch = "wasm32")]
pub fn enter_runtime() {}

#[cfg(target_arch = "wasm32")]
pub async fn yield_now() {
    use js_sys::Promise;
    use wasm_bindgen_futures::JsFuture;

    // Use a microtask or 0-ms timeout to yield to the browser event loop
    let promise = Promise::resolve(&wasm_bindgen::JsValue::NULL);
    let _ = JsFuture::from(promise).await;
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn yield_now() {
    tokio::task::yield_now().await;
}
