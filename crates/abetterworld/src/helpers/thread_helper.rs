#[macro_export]
macro_rules! spawn_detached_thread {
    ($body:block) => {{
        #[cfg(not(target_arch = "wasm32"))]
        {
            std::thread::spawn(move || $body)
        }

        #[cfg(target_arch = "wasm32")]
        {
            wasm_bindgen_futures::spawn_local(async move { $body });
        }
    }};
}
