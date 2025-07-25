use cgmath::MetricSpace;
// Optimized Tile Content System for Rust + WASM
use crossbeam_channel::bounded;
use reqwest::blocking::Client;
use std::sync::mpsc::{channel, sync_channel, SyncSender};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep};
use std::time::Duration;

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
    let max_loader_threads = 1;
    let (pager_sender, priortize_receiver) = channel::<Tile>();
    let (priortize_sender, loader_receiver) = sync_channel(max_loader_threads);

    // one client used for all downloads (is that good?)
    let client = Client::builder()
        .user_agent("abetterworld")
        .pool_max_idle_per_host(max_loader_threads + 1)
        .build()
        .unwrap();

    let mut tileset_pager = TileSetImporter::new(client.clone(), pager_sender, tile_manager);

    // Pager Thread
    {
        let my_camera_source = Arc::clone(&camera_source);
        thread::spawn(move || loop {
            let camera_data = if let Ok(camera) = my_camera_source.read() {
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

    // Priortize Tiles Thread
    // If the loader/worker threads are busy, then priortize the tiles in the backlog
    {
        thread::spawn(move || {
            let mut priortized_tiles = Vec::new();

            loop {
                let camera_data = if let Ok(camera) = camera_source.read() {
                    camera.refinement_data()
                } else {
                    log::warn!("Failed to acquire read lock on camera source");
                    continue;
                };

                let mut added_new_tiles = false;
                for tile in priortize_receiver.try_iter() {
                    if tile.state == TileState::ToLoad {
                        priortized_tiles.push(tile);
                        added_new_tiles = true;
                    }
                }

                if added_new_tiles {
                    // sort priortized tiles by distance to camera
                    priortized_tiles.sort_by(|a, b| {
                        let a_distance = camera_data.position.distance2(a.volume.center());
                        let b_distance = camera_data.position.distance2(b.volume.center());
                        a_distance
                            .partial_cmp(&b_distance)
                            .unwrap_or(std::cmp::Ordering::Greater)
                    });
                }

                let mut send_tiles = Vec::new();
                for tile in priortized_tiles.iter() {
                    if let Ok(()) = priortize_sender.try_send(tile.clone()) {
                        // it worked, so removed from the list
                        send_tiles.push(tile.id);
                    } else {
                        break; // channel is full, stop sending
                    }
                }

                // remove sent tiles from priortized list
                priortized_tiles.retain(|tile| !send_tiles.contains(&tile.id));
            }
        });
    }

    // Download/Decode Tile Thread Pool
    {
        thread::spawn(move || {
            let (task_sender, task_receiver) = bounded::<Tile>(max_loader_threads);

            // Fixed number of worker threads
            for _ in 0..max_loader_threads {
                let client_clone = client.clone();
                let sender_clone = main_thread_sender.clone();
                let task_receiver = task_receiver.clone();

                thread::spawn(move || {
                    while let Ok(mut tile) = task_receiver.recv() {
                        if tile.state == TileState::ToLoad {
                            std::thread::sleep(Duration::from_millis(500));
                            content_load(&client_clone, GOOGLE_API_KEY, &mut tile)
                                .unwrap_or_else(|e| log::error!("Failed to load tile: {}", e));
                            if matches!(tile.state, TileState::Decoded { .. }) {
                                sender_clone.send(tile).ok();
                            }
                        }
                    }
                });
            }

            // Backpressure point: blocks if queue is full
            for tile in loader_receiver.iter() {
                task_sender.send(tile).unwrap();
            }

            log::warn!("loader_receiver closed; exiting loader thread");
        });
    }

    Ok(())
}
