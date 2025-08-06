use std::{
    cell::OnceCell,
    sync::{atomic::AtomicUsize, Arc, Mutex},
};

use js_sys::Function;
use wasm_bindgen::prelude::*;
use wasm_bindgen_spawn::ThreadCreator;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &JsValue);
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn log_str(s: &str);
    #[wasm_bindgen(js_namespace = console)]
    fn error(s: &JsValue);
}

thread_local! {
    static THREAD_CREATOR: OnceCell<Arc<ThreadCreator>> = const { OnceCell::new() };
}

#[wasm_bindgen]
pub async fn init_wasm_module() {
    console_error_panic_hook::set_once();
    let thread_creator = match ThreadCreator::unready("pkg/example_bg.wasm", "pkg/example.js") {
        Ok(v) => v,
        Err(e) => {
            unsafe { log_str("Failed to create thread creator") };
            unsafe { error(&e) };
            return;
        }
    };
    let thread_creator = match thread_creator.ready().await {
        Ok(v) => v,
        Err(e) => {
            unsafe { log_str("Failed to create thread creator") };
            unsafe { error(&e) };
            return;
        }
    };
    THREAD_CREATOR.with(|cell| {
        let _ = cell.set(Arc::new(thread_creator));
    });
}

pub fn thread_creator() -> Arc<ThreadCreator> {
    THREAD_CREATOR.with(|cell| Arc::clone(cell.get().unwrap()))
}
