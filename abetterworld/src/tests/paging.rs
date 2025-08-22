#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{
        cache::init_tileset_cache,
        content::{start_pager, Tile, TileManager},
        decode::init,
        dynamics::init_camera,
        helpers::{channel::channel, PlatformAwait},
    };

    #[test]
    fn test_paging() {
        init_tileset_cache("../tilesets");

        let (_camera, debug_camera) = init_camera();

        let tile_content = Arc::new(TileManager::new());
        let debug_camera_source = Arc::new(debug_camera);

        let (loader_tx, render_rx) = channel::<Tile>(20);

        let _ = init();
        let _ = start_pager(debug_camera_source.clone(), tile_content.clone(), loader_tx);

        let mut tile_counter = 0;
        while tile_counter < 50 {
            match render_rx.recv().platform_await() {
                Ok(_tile) => {
                    tile_counter += 1;
                }
                Err(_) => break, // either empty or sender gone
            }
        }
        assert!(tile_counter > 0, "Expected to receive some tiles");
    }
}
