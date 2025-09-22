use crate::cache::init_wasm_indexdb_on_every_thread;
use crate::content::parser_iteration;
use crate::content::tiles;
use crate::content::tiles::load_content;
use crate::content::tiles_priority::priortize_loop;
use crate::content::Client;
use crate::content::Tile;
use crate::content::TileSetImporter;
use crate::helpers::channel::channel;
use crate::helpers::channel::{Receiver, Sender};
use crate::Source;
use crate::{content::TileManager, dynamics::Camera, helpers::AbwError};
use gloo_timers::future::TimeoutFuture;
use once_cell::sync::Lazy;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::{sync::Arc, time::Duration};
use wasm_bindgen_futures::spawn_local;

static ACTIVE_JOBS: Lazy<AtomicI32> = Lazy::new(|| AtomicI32::new(0));

pub fn start_pager(
    source: Source,
    camera_src: Arc<Camera>,
    tile_mgr: Arc<TileManager>,
    render_tx: Sender<Tile>,
) -> Result<(), AbwError> {
    spawn_local(async move {
        match init_wasm_indexdb_on_every_thread().await {
            Ok(_) => log::info!("Initialized IndexedDB"),
            Err(e) => log::error!("Failed to initialize IndexedDB: {:?}", e),
        }

        // run update_pager every 2 seconds
        let mut last_cam_gen = 0;
        loop {
            let new_gen = camera_src.generation();
            if new_gen != last_cam_gen {
                log::info!("Camera state has changed; updating pager");
                last_cam_gen = new_gen;

                if let Err(e) = update_pager(
                    source.clone(),
                    camera_src.clone(),
                    tile_mgr.clone(),
                    render_tx.clone(),
                )
                .map_err(|e| {
                    log::error!("Failed to update pager: {:?}", e);
                    e
                }) {
                    log::error!("update_pager error: {:?}", e);
                }
            }
            // wait 2 seconds
            TimeoutFuture::new(2000).await;
        }
    });

    Ok(())
}

fn update_pager(
    source: Source,
    camera_src: Arc<Camera>,
    tile_mgr: Arc<TileManager>,
    render_tx: Sender<Tile>,
) -> Result<(), AbwError> {
    if ACTIVE_JOBS
        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::debug!("update_pager already running; skipping");
        return Ok(());
    }

    const LOADER_THREADS: usize = 12;
    let (pager_tx, mut pager_rx) = channel::<Tile>(1000);
    let (loader_tx, mut loader_rx) = channel::<Tile>(LOADER_THREADS * 2);
    let client = build_client(LOADER_THREADS)?;

    let client_clone = client.clone();
    let pager_cam = Arc::clone(&camera_src);
    let tile_mgr = Arc::clone(&tile_mgr);
    let source_clone = source.clone();

    let pager_tx_clone = pager_tx.clone();
    spawn_local(async move {
        ACTIVE_JOBS.fetch_add(1, Ordering::SeqCst);

        log::info!("Starting pager");

        let mut pager = TileSetImporter::new(client_clone, pager_tx_clone, tile_mgr);

        // this should compare the current camera state generation to avoid redundant work
        let camera_data = pager_cam.refinement_data();
        if let Err(e) = parser_iteration(&source_clone, &camera_data, &mut pager).await {
            log::error!("Pager thread failed: {:?}", e);
        }

        ACTIVE_JOBS.fetch_sub(1, Ordering::SeqCst);
    });
    drop(pager_tx);

    let cam = Arc::clone(&camera_src);
    let mut loader_tx_clone = loader_tx.clone();
    spawn_local(async move {
        ACTIVE_JOBS.fetch_add(1, Ordering::SeqCst);

        log::info!("Starting prioritized loop");

        // this will run until the pager channel is closed
        if let Err(e) = priortize_loop(&cam, &mut pager_rx, &mut loader_tx_clone).await {
            log::info!("Prioritized loop thread ended: {:?}", e);
        }

        ACTIVE_JOBS.fetch_sub(1, Ordering::SeqCst);
    });
    drop(loader_tx);

    spawn_local(async move {
        ACTIVE_JOBS.fetch_add(1, Ordering::SeqCst);

        while let Ok(mut tile) = loader_rx.recv().await {
            let client_clone = client.clone();
            let mut render_tx_clone = render_tx.clone();
            let source_clone = source.clone();

            spawn_local(async move {
                ACTIVE_JOBS.fetch_add(1, Ordering::SeqCst);

                load_content(
                    &source_clone,
                    &client_clone,
                    &mut tile,
                    &mut render_tx_clone,
                )
                .await
                .unwrap_or_else(|e| {
                    log::error!("Failed to load content for tile {}: {:?}", tile.uri, e);
                });

                ACTIVE_JOBS.fetch_sub(1, Ordering::SeqCst);
            });
        }

        ACTIVE_JOBS.fetch_sub(1, Ordering::SeqCst);
    });

    ACTIVE_JOBS.fetch_sub(1, Ordering::SeqCst);
    Ok(())
}

fn build_client(threads: usize) -> Result<Client, AbwError> {
    Client::new(threads)
}
