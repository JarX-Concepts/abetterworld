#[cfg(test)]
mod tests {
    use std::sync::Arc;

    pub const GOOGLE_API_KEY: &str = "AIzaSyD526Czd1rD44BZE2d2R70-fBEdDdf6vZQ";
    pub const GOOGLE_API_URL: &str = "https://tile.googleapis.com/v1/3dtiles/root.json";

    use cgmath::Point3;

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

        let camera = init_camera(Point3::new(51.5074, -0.1278, 100.0)); // London

        let tile_content = Arc::new(TileManager::new());
        let debug_camera_source = Arc::new(camera);

        let (loader_tx, render_rx) = channel::<Tile>(20);

        let _ = init();
        let _ = start_pager(
            crate::Source::Google {
                key: GOOGLE_API_KEY.to_string(),
                url: GOOGLE_API_URL.to_string(),
            },
            debug_camera_source.clone(),
            tile_content.clone(),
            loader_tx,
        );

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
