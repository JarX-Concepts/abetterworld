// Optimized Tile Content System for Rust + WASM
use reqwest::blocking::Client;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep};
use std::time::Duration;
use threadpool::ThreadPool;

use crate::camera::Camera;
use crate::content::{Tile, TileState};
use crate::errors::AbwError;
use crate::tile_manager::TileManager;
use crate::tiles::content_load;
use crate::tilesets::TileSetImporter;

const GOOGLE_API_KEY: &str = "AIzaSyD526Czd1rD44BZE2d2R70-fBEdDdf6vZQ";
const GOOGLE_API_URL: &str = "https://tile.googleapis.com/v1/3dtiles/root.json";

fn wait_short_delay() {
    sleep(Duration::from_millis(10));
}

fn wait_longer_delay() {
    sleep(Duration::from_millis(1000));
}

pub fn start_pager(
    camera_source: Arc<RwLock<Camera>>,
    tile_manager: Arc<TileManager>,
    main_thread_sender: SyncSender<Tile>,
) -> Result<(), AbwError> {
    let max_loader_threads = 40;
    let (sender, receiver) = sync_channel(max_loader_threads * 2);
    let client = Client::builder()
        .user_agent("abetterworld")
        .pool_max_idle_per_host(max_loader_threads)
        .build()
        .unwrap();

    let mut tileset_pager = TileSetImporter::new(client.clone(), sender, tile_manager);
    {
        thread::spawn(move || loop {
            let camera_data = if let Ok(camera) = camera_source.read() {
                camera.refinement_data()
            } else {
                log::warn!("Failed to acquire read lock on camera source");
                continue;
            };

            tileset_pager
                .go(&camera_data, GOOGLE_API_URL, GOOGLE_API_KEY)
                .err()
                .map(|e| log::error!("Failed to import tileset: {}", e));

            wait_short_delay();
        });
    }

    {
        thread::spawn(move || {
            let pool = ThreadPool::new(max_loader_threads);
            loop {
                match receiver.recv() {
                    Ok(mut tile) => {
                        let client_clone = client.clone();
                        let sender_clone = main_thread_sender.clone();
                        pool.execute(move || {
                            if tile.state == TileState::ToLoad {
                                content_load(&client_clone, GOOGLE_API_KEY, &mut tile)
                                    .unwrap_or_else(|e| log::error!("Failed to load tile: {}", e));

                                if matches!(tile.state, TileState::Decoded { .. }) {
                                    // you're almost there
                                    if let Err(e) = sender_clone.send(tile) {
                                        log::error!("Failed to send tile to main thread: {}", e);
                                    }
                                } else {
                                    log::warn!("Tile not in decoded state: {}", tile.uri);
                                }
                            } else {
                                log::warn!("Tile not in load state: {}", tile.uri);
                            }
                        });
                    }
                    Err(_) => {
                        log::warn!("Receiver closed");
                        break;
                    }
                }
            }
        });
    }

    Ok(())
}
