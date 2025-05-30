#[cfg(target_arch = "wasm32")]
mod wasm_tests {
    use crate::cache::{get_tileset_cache, init_tileset_cache};
    use bytes::Bytes;
    use js_sys::Math;
    use log::info;
    use wasm_bindgen_test::*;

    fn random_id(len: usize) -> String {
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        let mut out = String::with_capacity(len);
        for _ in 0..len {
            let idx = (Math::random() * CHARSET.len() as f64) as usize;
            out.push(CHARSET[idx] as char);
        }
        out
    }

    wasm_bindgen_test_configure!(run_in_browser);

    fn random_key(prefix: &str, i: usize) -> String {
        format!("{}-{}", prefix, random_id(8))
    }

    #[wasm_bindgen_test]
    async fn test_indexeddb_stress_lifecycle() {
        console_log::init_with_level(log::Level::Info).ok();
        init_tileset_cache().await;

        println!("Starting IndexedDB stress test...");

        let Some(cache) = get_tileset_cache() else {
            panic!("Cache not initialized");
        };

        let content_type = "application/octet-stream";
        let base_value = Bytes::from_static(b"stress-test-payload");

        // Insert a bunch of entries
        info!("Inserting 1000 entries...");
        for i in 0..1000 {
            let key = random_key("stress", i);
            let val = Bytes::from(vec![(i % 255) as u8; 64]);
            cache
                .insert(key.clone(), content_type.to_string(), val.clone())
                .await;
        }

        // Insert a known key
        let control_key = "stress-control-key";
        let control_value = Bytes::from_static(b"CONTROL-DATA-123456");
        cache
            .insert(
                control_key.to_string(),
                content_type.to_string(),
                control_value.clone(),
            )
            .await;

        // Evict memory
        {
            let mut map = cache.map.lock().await;
            map.clear();
        }

        // Restore control key
        info!("Fetching control key from IndexedDB after memory clear...");
        let result = cache.get(control_key).await;
        assert!(result.is_some(), "Control key should be recoverable");
        let (ct, val) = result.unwrap();
        assert_eq!(ct, content_type);
        assert_eq!(val, control_value);

        // Insert edge case entries
        info!("Inserting edge-case entries...");
        let empty_key = "empty-key";
        let utf_key = "unicode-ключ🗝️";
        let empty_payload = Bytes::new();

        cache
            .insert(
                empty_key.to_string(),
                content_type.to_string(),
                empty_payload.clone(),
            )
            .await;
        cache
            .insert(
                utf_key.to_string(),
                content_type.to_string(),
                base_value.clone(),
            )
            .await;

        let (ct_empty, val_empty) = cache.get(empty_key).await.unwrap();
        assert_eq!(ct_empty, content_type);
        assert_eq!(val_empty, empty_payload);

        let (ct_utf, val_utf) = cache.get(utf_key).await.unwrap();
        assert_eq!(ct_utf, content_type);
        assert_eq!(val_utf, base_value);

        // Overwrite an existing key
        info!("Overwriting existing key...");
        let new_val = Bytes::from_static(b"new-overwritten-value");
        cache
            .insert(
                control_key.to_string(),
                content_type.to_string(),
                new_val.clone(),
            )
            .await;

        let (ct_overwritten, val_overwritten) = cache.get(control_key).await.unwrap();
        assert_eq!(ct_overwritten, content_type);
        assert_eq!(val_overwritten, new_val);

        // Cleanup and verify
        info!("Cleaning up IndexedDB...");
        cache.clear().await;

        info!("Verifying post-cleanup...");
        let post_cleanup = cache.get(control_key).await;
        assert!(post_cleanup.is_none(), "Control key should be deleted");

        info!("✅ Intense IndexedDB + LRU cache test passed");
    }
}
