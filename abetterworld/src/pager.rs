use cgmath::MetricSpace;
use crossbeam_channel::{bounded, select, unbounded};
use reqwest::blocking::Client;
use std::sync::RwLock;
use std::{sync::Arc, thread, time::Duration};
use wgpu::naga::back;

use crate::camera::Camera;
use crate::content::{Tile, TileState};
use crate::errors::AbwError;
use crate::tile_manager::TileManager;
use crate::tiles::content_load;
use crate::tilesets::TileSetImporter;

const GOOGLE_API_KEY: &str = "AIzaSyD526Czd1rD44BZE2d2R70-fBEdDdf6vZQ";
const GOOGLE_API_URL: &str = "https://tile.googleapis.com/v1/3dtiles/root.json";

pub fn start_pager(
    camera_src: Arc<Camera>,
    tile_mgr: Arc<TileManager>,
    render_tx: crossbeam_channel::Sender<Tile>,
) -> Result<(), AbwError> {
    const LOADER_THREADS: usize = 20;
    // unbounded: pager -> prioritizer
    let (pager_tx, pager_rx) = unbounded::<Tile>();
    // bounded:   prioritizer -> workers  (back-pressure)
    let (loader_tx, loader_rx) = bounded::<Tile>(LOADER_THREADS * 2);

    // ---------- 1. Pager (discovers tiles) ----------
    {
        let cam = Arc::clone(&camera_src);
        let mut pager =
            TileSetImporter::new(build_client(LOADER_THREADS)?, pager_tx.clone(), tile_mgr);

        thread::spawn(move || {
            let mut last_cam_gen = 0;
            loop {
                let new_gen = cam.generation();
                if new_gen != last_cam_gen {
                    let camera_data = cam.refinement_data();

                    if let Err(e) = pager.go(&camera_data, GOOGLE_API_URL, GOOGLE_API_KEY) {
                        log::error!("tileset import failed: {e}");
                    }

                    last_cam_gen = new_gen;
                } else {
                    // No camera movement, sleep briefly to avoid busy-waiting
                    thread::sleep(Duration::from_millis(10));
                }
            }
        });
    }

    // ---------- 2. Prioritizer ----------
    {
        let cam = Arc::clone(&camera_src);
        thread::spawn(move || {
            let mut backlog: Vec<Tile> = Vec::new();
            let mut last_cam_gen = 0;

            loop {
                let mut did_nothing_iter = true;

                if backlog.is_empty() {
                    // No backlog, block wait for a tile
                    let t = pager_rx.recv();
                    if let Ok(tile) = t {
                        backlog.push(tile);
                        did_nothing_iter = false;
                    }
                }

                // ingest new tiles -------------------------------------------------
                for t in pager_rx.try_iter() {
                    if t.state == TileState::ToLoad {
                        backlog.push(t);
                        did_nothing_iter = false;
                    }
                }

                // detect camera movement ------------------------------------------
                let new_gen = cam.generation();
                if new_gen != last_cam_gen {
                    let camera_data = cam.refinement_data();
                    backlog.sort_unstable_by(|a, b| {
                        let da = camera_data.position.distance2(a.volume.center());
                        let db = camera_data.position.distance2(b.volume.center());
                        db.partial_cmp(&da).unwrap()
                    });
                    last_cam_gen = new_gen;
                }

                // feed workers -----------------------------------------------------
                while let Some(tile) = backlog.last() {
                    // ≈ cheapest (small dist) at back
                    if loader_tx.try_send(tile.clone()).is_ok() {
                        backlog.pop();
                        did_nothing_iter = false;
                    } else {
                        break;
                    }
                }

                if did_nothing_iter {
                    // No new tiles or camera movement, sleep briefly to avoid busy-waiting
                    thread::sleep(Duration::from_millis(10));
                }
            }
        });
    }

    // ---------- 3. Workers ----------
    {
        let cli = build_client(LOADER_THREADS)?;

        for _ in 0..LOADER_THREADS {
            let client_clone = cli.clone();
            let render_time = render_tx.clone();
            let rx = loader_rx.clone();

            thread::spawn(move || {
                for mut tile in rx.iter() {
                    if tile.state == TileState::ToLoad {
                        if let Err(e) = content_load(&client_clone, GOOGLE_API_KEY, &mut tile) {
                            log::error!("load failed: {e}");
                            continue;
                        }
                        if matches!(tile.state, TileState::Decoded { .. }) {
                            let _ = render_time.send(tile);
                        }
                    }
                }
            });
        }
    }

    Ok(())
}

fn build_client(threads: usize) -> Result<Client, AbwError> {
    Client::builder()
        .user_agent("abetterworld")
        .pool_max_idle_per_host(threads + 1)
        .build()
        .map_err(|e| AbwError::Network(format!("Failed to build HTTP client: {e}")))
}
