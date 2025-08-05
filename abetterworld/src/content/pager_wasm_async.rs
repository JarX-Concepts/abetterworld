use crate::content::Client;
use crate::content::Tile;
use crate::helpers::channel::channel;
use crate::helpers::channel::Sender;
use crate::{content::TileManager, helpers::AbwError, render::Camera};
use std::sync::Arc;
use wasm_bindgen_futures::spawn_local;

pub fn start_pager(
    _camera_src: Arc<Camera>,
    _tile_mgr: Arc<TileManager>,
    _render_tx: Sender<Tile>,
) -> Result<(), AbwError> {
    Ok(())
}

pub fn update_pager(
    camera_src: Arc<Camera>,
    tile_mgr: Arc<TileManager>,
    render_tx: Sender<Tile>,
) -> Result<(), AbwError> {
    const LOADER_THREADS: usize = 1;
    let (pager_tx, mut pager_rx) = channel::<Tile>(1000);
    let (loader_tx, mut loader_rx) = channel::<Tile>(LOADER_THREADS * 2);
    let client = build_client(LOADER_THREADS)?;

    {
        let client_clone = client.clone();
        let pager_cam = Arc::clone(&camera_src);
        let tile_mgr = Arc::clone(&tile_mgr);

        spawn_local(async move {
            use crate::content::parser_thread;
            log::info!("Starting parser thread");
            if let Err(e) =
                parser_thread(pager_cam, tile_mgr, pager_tx.clone(), client_clone, false).await
            {
                log::error!("Pager thread failed: {:?}", e);
            }
            log::info!("Done parser thread");
            drop(pager_tx);
        });

        let cam = Arc::clone(&camera_src);

        let mut loader_tx_clone = loader_tx.clone();
        spawn_local(async move {
            log::info!("Start priortize_loop");
            use crate::content::tiles_priority::priortize_loop;
            // this will run until the pager channel is closed
            if let Err(e) = priortize_loop(&cam, &mut pager_rx, &mut loader_tx_clone, false).await {
                log::error!("Pager thread failed: {:?}", e);
            }

            drop(loader_tx_clone);

            log::info!("Done priortize_loop");
        });

        {
            let client_clone = client.clone();
            let mut render_tx_clone = render_tx.clone();

            spawn_local(async move {
                log::info!("Start worker thread");
                use crate::content::tiles::wait_and_load_content;

                if let Err(e) =
                    wait_and_load_content(&client_clone, &mut loader_rx, &mut render_tx_clone).await
                {
                    log::error!("Worker failed: {:?}", e);
                }

                log::info!("Done worker thread");
            });
        }
    }

    Ok(())
}

fn build_client(threads: usize) -> Result<Client, AbwError> {
    Client::new(threads)
}
