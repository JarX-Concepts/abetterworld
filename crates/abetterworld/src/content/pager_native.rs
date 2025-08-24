use crate::{
    content::{
        parser_thread, tiles::wait_and_load_content, tiles_priority::priortize_loop, Client, Tile,
        TileManager,
    },
    dynamics::Camera,
    helpers::{
        channel::{channel, Receiver, Sender},
        AbwError, PlatformAwait,
    },
    Source,
};
use std::{sync::Arc, thread, time::Duration};

pub fn start_pager(
    source: Source,
    camera_src: Arc<Camera>,
    tile_mgr: Arc<TileManager>,
    render_tx: Sender<Tile>,
) -> Result<(), AbwError> {
    const LOADER_THREADS: usize = 12;
    // unbounded: pager -> prioritizer
    let (pager_tx, mut pager_rx) = channel::<Tile>(1000);
    // bounded:   prioritizer -> workers  (back-pressure)
    let (mut loader_tx, loader_rx) = channel::<Tile>(LOADER_THREADS * 2);
    let client = build_client(LOADER_THREADS)?;

    // ---------- 1. Pager (discovers tiles) ----------
    {
        let client_clone = client.clone();
        let pager_cam = Arc::clone(&camera_src);
        let source_clone = source.clone();
        thread::spawn(move || {
            parser_thread(
                &source_clone,
                pager_cam,
                tile_mgr,
                pager_tx,
                client_clone,
                true,
            )
            .platform_await()
            .expect("Failed to start parser thread");
        });
    }

    // ---------- 2. Prioritizer ----------
    {
        let cam = Arc::clone(&camera_src);
        thread::spawn(move || {
            priortize_loop(&cam, &mut pager_rx, &mut loader_tx, true)
                .platform_await()
                .expect("Failed to run prioritizer loop");
        });
    }

    // ---------- 3. Workers ----------
    {
        for _ in 0..LOADER_THREADS {
            let client_clone = client.clone();
            let mut render_time = render_tx.clone();
            let mut rx = loader_rx.clone();
            let source_clone = source.clone();

            thread::spawn(move || {
                wait_and_load_content(&source_clone, &client_clone, &mut rx, &mut render_time)
                    .platform_await()
                    .expect("Failed to load content in worker thread");
            });
        }
    }

    Ok(())
}

fn build_client(threads: usize) -> Result<Client, AbwError> {
    Client::new(threads)
}

pub fn import_renderables(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    content: &Arc<TileManager>,
    receiver: &mut Receiver<Tile>,
    budget: Duration,
) -> Result<bool, AbwError> {
    use std::time::Instant;

    let deadline = Instant::now() + budget;
    let mut needs_update = false;

    while Instant::now() < deadline {
        match receiver.try_recv() {
            Ok(mut tile) => {
                use crate::content::tiles;

                let new_tile = tiles::content_render_setup(device, queue, layout, &mut tile)?;
                content.add_renderable(new_tile);
                needs_update = true;
            }
            Err(_) => break, // nothing left
        }
    }
    Ok(needs_update)
}
