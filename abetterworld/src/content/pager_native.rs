use crate::camera::Camera;
use crate::content::Tile;
use crate::download_client::Client;
use crate::errors::AbwError;
use crate::platform::PlatformAwait;
use crate::tile_manager::TileManager;
use crate::tiles::wait_and_load_content;
use crate::tiles_priority::priortize_loop;
use crate::tilesets::parser_thread;
use crossbeam_channel::{bounded, unbounded};
use std::{sync::Arc, thread};

pub fn start_pager(
    camera_src: Arc<Camera>,
    tile_mgr: Arc<TileManager>,
    render_tx: crossbeam_channel::Sender<Tile>,
) -> Result<(), AbwError> {
    const LOADER_THREADS: usize = 1;
    // unbounded: pager -> prioritizer
    let (pager_tx, pager_rx) = unbounded::<Tile>();
    // bounded:   prioritizer -> workers  (back-pressure)
    let (loader_tx, loader_rx) = bounded::<Tile>(LOADER_THREADS * 2);
    let client = build_client(LOADER_THREADS)?;

    // ---------- 1. Pager (discovers tiles) ----------
    {
        let client_clone = client.clone();
        let pager_cam = Arc::clone(&camera_src);
        thread::spawn(move || {
            parser_thread(pager_cam, tile_mgr, pager_tx, client_clone, true)
                .platform_await()
                .expect("Failed to start parser thread");
        });
    }

    // ---------- 2. Prioritizer ----------
    {
        let cam = Arc::clone(&camera_src);
        thread::spawn(move || {
            priortize_loop(&cam, &pager_rx, &loader_tx);
        });
    }

    // ---------- 3. Workers ----------
    {
        for _ in 0..LOADER_THREADS {
            let client_clone = client.clone();
            let render_time = render_tx.clone();
            let rx = loader_rx.clone();

            thread::spawn(move || {
                wait_and_load_content(&client_clone, &rx, &render_time)
                    .platform_await()
                    .expect("Failed to load content in worker thread");
            });
        }
    }

    Ok(())
}

pub fn pager_iter(
    camera_src: Arc<Camera>,
    tile_mgr: Arc<TileManager>,
    render_tx: crossbeam_channel::Sender<Tile>,
) -> Result<(), AbwError> {
    // This is a blocking iterator that runs the pager loop
    start_pager(camera_src, tile_mgr, render_tx)?;

    // Keep the main thread alive to allow async tasks to run
    loop {
        thread::park();
    }
}

fn build_client(threads: usize) -> Result<Client, AbwError> {
    Client::new(threads)
}
