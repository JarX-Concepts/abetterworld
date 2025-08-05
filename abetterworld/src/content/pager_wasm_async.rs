use crate::cache::init_wasm_indexdb_on_every_thread;
use crate::content::tiles::load_content;
use crate::content::Client;
use crate::content::Tile;
use crate::helpers::channel::channel;
use crate::helpers::channel::Sender;
use crate::{content::TileManager, helpers::AbwError, render::Camera};
use once_cell::sync::Lazy;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use wasm_bindgen_futures::spawn_local;

static PAGER_RUNNING: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

static ACTIVE_JOBS: Lazy<AtomicI32> = Lazy::new(|| AtomicI32::new(0));

pub fn start_pager(
    _camera_src: Arc<Camera>,
    _tile_mgr: Arc<TileManager>,
    _render_tx: Sender<Tile>,
) -> Result<(), AbwError> {
    spawn_local(async move {
        match init_wasm_indexdb_on_every_thread().await {
            Ok(_) => log::info!("Initialized IndexedDB"),
            Err(e) => log::error!("Failed to initialize IndexedDB: {:?}", e),
        }
    });

    Ok(())
}

pub fn update_pager(
    camera_src: Arc<Camera>,
    tile_mgr: Arc<TileManager>,
    render_tx: Sender<Tile>,
) -> Result<(), AbwError> {
    if ACTIVE_JOBS
        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        log::warn!("update_pager already running; skipping");
        return Ok(());
    }

    const LOADER_THREADS: usize = 12;
    let (pager_tx, mut pager_rx) = channel::<Tile>(1000);
    let (loader_tx, mut loader_rx) = channel::<Tile>(LOADER_THREADS * 2);
    let client = build_client(LOADER_THREADS)?;

    let client_clone = client.clone();
    let pager_cam = Arc::clone(&camera_src);
    let tile_mgr = Arc::clone(&tile_mgr);

    let pager_tx_clone = pager_tx.clone();
    spawn_local(async move {
        use crate::content::parser_thread;

        ACTIVE_JOBS.fetch_add(1, Ordering::SeqCst);

        if let Err(e) =
            parser_thread(pager_cam, tile_mgr, pager_tx_clone, client_clone, false).await
        {
            log::error!("Pager thread failed: {:?}", e);
        }

        ACTIVE_JOBS.fetch_sub(1, Ordering::SeqCst);
    });
    drop(pager_tx);

    let cam = Arc::clone(&camera_src);
    let mut loader_tx_clone = loader_tx.clone();
    spawn_local(async move {
        use crate::content::tiles_priority::priortize_loop;

        ACTIVE_JOBS.fetch_add(1, Ordering::SeqCst);

        // this will run until the pager channel is closed
        if let Err(e) = priortize_loop(&cam, &mut pager_rx, &mut loader_tx_clone, false).await {
            log::error!("Prioritized loop thread failed: {:?}", e);
        }

        ACTIVE_JOBS.fetch_sub(1, Ordering::SeqCst);
    });
    drop(loader_tx);

    spawn_local(async move {
        ACTIVE_JOBS.fetch_add(1, Ordering::SeqCst);

        while let Ok(mut tile) = loader_rx.recv().await {
            let client_clone = client.clone();
            let mut render_tx_clone = render_tx.clone();

            spawn_local(async move {
                ACTIVE_JOBS.fetch_add(1, Ordering::SeqCst);

                load_content(&client_clone, &mut tile, &mut render_tx_clone)
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
