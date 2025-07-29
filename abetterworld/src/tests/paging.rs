#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crossbeam_channel::bounded;

    use crate::{
        cache::init_tileset_cache,
        camera::init_camera,
        content::{Tile, TileState},
        decode::init,
        pager::start_pager,
        tile_manager::TileManager,
    };

    #[test]
    fn test_paging() {
        init_tileset_cache();

        let (_camera, debug_camera) = init_camera();

        let tile_content = Arc::new(TileManager::new());
        let debug_camera_source = Arc::new(debug_camera);

        let (loader_tx, render_rx) = bounded::<Tile>(20);

        let _ = init();
        let _ = start_pager(debug_camera_source.clone(), tile_content.clone(), loader_tx);

        let mut tile_counter = 0;
        while tile_counter < 200 {
            match render_rx.recv() {
                Ok(tile) => {
                    if let TileState::Decoded { .. } = tile.state {
                        tile_counter += 1;
                    }
                }
                Err(crossbeam_channel::RecvError) => break, // either empty or sender gone
            }
        }
        assert!(tile_counter > 0, "Expected to receive some tiles");
    }
}
