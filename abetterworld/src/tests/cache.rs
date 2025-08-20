#[cfg(test)]
mod tests {
    use crate::cache::{get_tileset_cache, init_tileset_cache};
    use crate::helpers::{hash_uri, PlatformAwait};

    use bytes::Bytes;
    use std::fs;
    use std::path::Path;

    #[test]
    fn test_insert_get_lru_disk_roundtrip() {
        init_tileset_cache();

        let cache = get_tileset_cache();
        let base_key = "test-key";
        let content_type = "application/octet-stream";
        let value = Bytes::from_static(b"hello-payload");

        // Insert a key
        if let Err(e) = cache
            .insert(
                base_key.to_string(),
                content_type.to_string(),
                value.clone(),
            )
            .platform_await()
        {
            eprintln!("Failed to insert into cache: {}", e);
            panic!("Cache insert failed");
        }

        // Get from memory
        let result = cache.get(base_key).platform_await().unwrap();
        assert!(result.is_some());
        let (ct, val) = result.unwrap();
        assert_eq!(ct, content_type);
        assert_eq!(val, value);

        // Insert many additional keys to exceed LRU capacity
        for i in 0..1024 {
            let key = format!("key-{}", i);
            let val = Bytes::from(vec![i as u8; 32]);
            if let Err(e) = cache
                .insert(key.clone(), "application/test".to_string(), val.clone())
                .platform_await()
            {
                eprintln!("Failed to insert into cache: {}", e);
                panic!("Cache insert failed");
            }

            let found = cache.get(&key).platform_await().unwrap();
            assert!(found.is_some(), "Expected to find {}", key);
        }

        let in_memory = {
            let id = hash_uri(&base_key);
            cache.map.get(id)
        };
        assert!(
            in_memory.is_none(),
            "Expected base_key to be evicted from memory"
        );

        // But it should still be on disk
        let result = cache.get(base_key).platform_await().unwrap();
        assert!(
            result.is_some(),
            "Expected base_key to be recovered from disk"
        );

        let (ct_disk, val_disk) = result.unwrap();
        assert_eq!(ct_disk, content_type);
        assert_eq!(val_disk, value);

        // Clean up disk file
        let disk_path = cache.disk_path_for(base_key);
        if Path::new(&disk_path).exists() {
            fs::remove_file(disk_path).unwrap();
        }

        // Clean up spurious disk writes
        for i in 0..1024 {
            let key = format!("key-{}", i);
            let path = cache.disk_path_for(&key);
            if Path::new(&path).exists() {
                let _ = fs::remove_file(path);
            }
        }
    }
}
