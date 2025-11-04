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

#[cfg(target_arch = "wasm32")]
pub async fn sleep_ms(ms: i32) {
    use wasm_bindgen_futures::JsFuture;

    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .unwrap();
    });
    let _ = JsFuture::from(promise).await;
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn sleep_ms(ms: i32) {
    use std::{thread::sleep, time::Duration};
    sleep(Duration::from_millis(ms as u64));
}
