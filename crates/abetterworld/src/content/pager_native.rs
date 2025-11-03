use crate::{
    content::{parser_thread, tiles::wait_and_load_content, Client, TilePipelineMessage},
    dynamics::Camera,
    helpers::{
        channel::{channel, Sender},
        enter_runtime, AbwError, PlatformAwait,
    },
    set_thread_name, spawn_detached_thread, Source,
};
use std::sync::Arc;

pub fn start_pager(
    source: Source,
    camera_src: Arc<Camera>,
    render_tx: Sender<TilePipelineMessage>,
) -> Result<(), AbwError> {
    const LOADER_THREADS: usize = 12;
    // unbounded: pager -> prioritizer
    let (mut loader_tx, loader_rx) = channel::<TilePipelineMessage>(LOADER_THREADS);
    let client = build_client(LOADER_THREADS)?;

    // ---------- 1. Pager (discovers tiles) ----------
    {
        let client_clone = client.clone();
        let pager_cam = Arc::clone(&camera_src);
        let source_clone = source.clone();
        spawn_detached_thread!({
            set_thread_name!("Pager");

            parser_thread(&source_clone, pager_cam, &mut loader_tx, client_clone, true)
                .expect("Failed to start parser thread");
        });
    }

    // ---------- 2. Workers ----------
    {
        for _ in 0..LOADER_THREADS {
            let client_clone = client.clone();
            let mut render_time = render_tx.clone();
            let mut rx = loader_rx.clone();

            spawn_detached_thread!({
                set_thread_name!("Download/Decode Worker");

                let _enter = enter_runtime();
                wait_and_load_content(&client_clone, &mut rx, &mut render_time)
                    .platform_await()
                    .await
                    .expect("Failed to load content in worker thread");
            });
        }
    }

    Ok(())
}

pub fn build_client(threads: usize) -> Result<Client, AbwError> {
    Client::new(threads)
}
