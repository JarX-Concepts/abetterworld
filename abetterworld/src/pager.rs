// Optimized Tile Content System for Rust + WASM
use reqwest::blocking::Client;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep};
use std::time::Duration;
use threadpool::ThreadPool;

use crate::camera::Camera;
use crate::content::TileState;
use crate::errors::AbwError;
use crate::tile_manager::TileManager;
use crate::tiles::content_load;
use crate::tilesets::TileSetImporter;

const GOOGLE_API_KEY: &str = "AIzaSyDrSNqujmAmhhZtenz6MEofEuITd3z0JM0";
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
    main_thread_sender: SyncSender<crate::content::Tile>,
) -> Result<(), AbwError> {
    let max_threads = 20;
    let (sender, receiver) = sync_channel(max_threads * 2);

    let mut tileset_pager = TileSetImporter::new(sender, tile_manager);
    {
        thread::spawn(|| loop {
            let camera_data = if let Ok(camera) = camera_source.read() {
                camera.refinement_data()
            } else {
                log::warn!("Failed to acquire read lock on camera source");
                continue;
            };

            tileset_pager.go(&camera_data, GOOGLE_API_URL, GOOGLE_API_KEY);

            wait_short_delay();
        });
    }

    {
        thread::spawn(move || {
            let pool = ThreadPool::new(max_threads);
            let client = Client::new();

            loop {
                match receiver.recv() {
                    Ok(tile) => {
                        pool.execute(move || {
                            if tile.state == TileState::ToLoad {
                                let loaded_tile = content_load(&client, GOOGLE_API_KEY, &tile)
                                    .unwrap_or_else(|e| log::error!("Failed to load tile: {}", e));

                                if matches!(loaded_tile.state, TileState::Decoded { .. }) {
                                    if let Err(e) = main_thread_sender.send(loaded_tile) {
                                        log::error!("Failed to send tile to main thread: {}", e);
                                    }
                                } else {
                                    log::warn!("Tile not in decoded state: {}", loaded_tile.uri);
                                }
                            }
                        });
                    }
                    Err(_) => {
                        log::warn!("Receiver closed");
                        break;
                    }
                }

                wait_short_delay(); // Optional
            }
        });
    }

    Ok(())
}
