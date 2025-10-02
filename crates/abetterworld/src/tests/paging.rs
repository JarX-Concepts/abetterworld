#[cfg(test)]
mod tests {
    use std::sync::Arc;

    pub const GOOGLE_API_KEY: &str = "AIzaSyD526Czd1rD44BZE2d2R70-fBEdDdf6vZQ";
    pub const GOOGLE_API_URL: &str = "https://tile.googleapis.com/v1/3dtiles/root.json";

    use cgmath::Point3;

    use crate::{
        cache::{get_tileset_cache, init_tileset_cache},
        content::{
            pager_native::build_client, parser_iteration, Tile, TileManager, TileSetImporter,
        },
        decode::init,
        dynamics::init_camera,
        helpers::{channel::channel, PlatformAwait},
    };

    #[test]
    fn test_paging() {
        init_tileset_cache("../tilesets");
        let cache = get_tileset_cache();
        cache.clear().expect("Failed to clear cache");

        let camera = init_camera(Point3::new(34.4208, -119.6982, 6_378_137.0 * 2.0)); // Santa Barbara

        let tile_content = Arc::new(TileManager::new());
        let debug_camera_source = Arc::new(camera);
        debug_camera_source.update();

        let (loader_tx, render_rx) = channel::<Tile>(200000);

        let client = build_client(4).expect("Failed to build client");

        eprintln!("Starting paging test");

        let _ = init();
        let mut pager = TileSetImporter::new(client, loader_tx.clone(), tile_content);

        let _ = parser_iteration(
            &crate::Source::Google {
                key: GOOGLE_API_KEY.to_string(),
                url: GOOGLE_API_URL.to_string(),
            },
            &debug_camera_source.clone().refinement_data(),
            &mut pager,
        )
        .platform_await()
        .expect("Failed to load content in worker thread");

        drop(loader_tx); // close sender so we can finish receiving */
        let mut tile_counter = 0;
        loop {
            match render_rx.try_recv() {
                Ok(_tile) => {
                    tile_counter += 1;
                }
                Err(_) => break, // either empty or sender gone
            }
        }

        eprintln!("Received {} tiles", tile_counter);

        assert!(tile_counter > 0, "Expected to receive some tiles");
    }
}
