use crate::cache::init_wasm_indexdb_on_every_thread;
use crate::content::tiles_priority::{priortize, Pri};
use crate::content::{
    go, Gen, ParsingState, TileContent, TileKey, TileManager, TileMessage, TileSourceContent,
    TileSourceContentState,
};
use crate::dynamics::CameraRefinementData;
use crate::helpers::{sleep_ms, yield_now, PlatformAwait};
use crate::{
    content::{tiles::wait_and_load_content, Client, TilePipelineMessage},
    dynamics::Camera,
    helpers::{
        channel::{channel, Sender},
        enter_runtime, AbwError,
    },
    set_thread_name, spawn_detached_thread, Source,
};
use std::sync::Arc;
use tracing::{event, Level};

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
        let mut render_time = render_tx.clone();
        spawn_detached_thread!({
            set_thread_name!("Pager");

            #[cfg(target_arch = "wasm32")]
            match init_wasm_indexdb_on_every_thread().await {
                Ok(_) => event!(Level::INFO, "Initialized IndexedDB"),
                Err(e) => event!(Level::ERROR, "Failed to initialize IndexedDB: {:?}", e),
            }

            let fut = parser_thread(
                &source_clone,
                pager_cam,
                &mut loader_tx,
                &mut render_time,
                client_clone,
            );

            // wasm only
            #[cfg(target_arch = "wasm32")]
            fut.await.expect("Failed to start parser thread");

            // native only
            #[cfg(not(target_arch = "wasm32"))]
            fut.platform_await().expect("Failed to start parser thread");
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

                let fut = wait_and_load_content(&client_clone, &mut rx, &mut render_time);

                // wasm only
                #[cfg(target_arch = "wasm32")]
                fut.await.expect("Failed to load content in worker thread");

                // native only
                #[cfg(not(target_arch = "wasm32"))]
                fut.platform_await()
                    .expect("Failed to load content in worker thread");
            });
        }
    }

    Ok(())
}

pub fn send_load_tile(
    tile_src: &Pri<'_>,
    pager_tx: &mut Sender<TilePipelineMessage>,
    gen: Gen,
) -> Result<(), AbwError> {
    let tile = TileContent {
        uri: tile_src.tile_content.uri.clone(),
        state: crate::content::types::TileState::ToLoad,
    };
    pager_tx.try_send(TilePipelineMessage::Load((
        TileMessage {
            key: tile_src.tile_content.key,
            gen: gen,
        },
        tile,
    )))
}

pub fn send_unload_tile(
    id: TileKey,
    pager_tx: &mut Sender<TilePipelineMessage>,
    gen: Gen,
) -> Result<(), AbwError> {
    pager_tx.try_send(TilePipelineMessage::Unload(TileMessage {
        key: id,
        gen: gen,
    }))
}

pub fn send_update_tile(
    tile_src: &Pri<'_>,
    pager_tx: &mut Sender<TilePipelineMessage>,
    gen: Gen,
) -> Result<(), AbwError> {
    if let Some(tile_info) = &tile_src.tile_info {
        return pager_tx.try_send(TilePipelineMessage::Update((
            TileMessage {
                key: tile_src.tile_content.key,
                gen: gen,
            },
            tile_info.clone(),
        )));
    }
    Err(AbwError::TileLoading("No tile info to update".into()))
}

pub fn parser_iteration(
    source: &Source,
    client: &Client,
    camera_data: &CameraRefinementData,
    root: &mut Option<TileSourceContent>,
    pipeline_state: &TileManager,
    decoder_tx: &mut Sender<TilePipelineMessage>,
    renderer_tx: &mut Sender<TilePipelineMessage>,
    gen: Gen,
) -> Result<ParsingState, AbwError> {
    let mut parsing_state = go(source, client, camera_data, root)?;

    if let Some(tile) = root {
        if let Some(TileSourceContentState::LoadedTileSet { permanent, .. }) = &tile.loaded {
            if let Some(permanent_root) = permanent.as_ref() {
                if let Some(root) = &permanent_root.root {
                    // gather priority tiles
                    let mut priority_list: Vec<Pri> = Vec::new();

                    priortize(pipeline_state, camera_data, root, &mut priority_list)?;

                    // send as many as we can into the pipeline
                    for pri in priority_list.iter() {
                        if !pipeline_state.is_tile_loaded(pri.tile_content.key) {
                            if let Err(_err) = send_load_tile(pri, decoder_tx, gen) {
                                parsing_state = ParsingState::Instable;
                                // the channel is full, we will try again next time
                                break;
                            }

                            pipeline_state.mark_tile_loaded(pri.tile_content.key);
                        }
                    }

                    for pri in priority_list.iter() {
                        if let Some(tile_info) = &pri.tile_info {
                            if !pipeline_state.compare_tile_info(pri.tile_content.key, tile_info) {
                                if let Err(_err) = send_update_tile(pri, renderer_tx, gen) {
                                    parsing_state = ParsingState::Instable;

                                    // the channel is full, we will try again next time
                                    break;
                                }

                                pipeline_state.add_or_update_tile_info(
                                    pri.tile_content.key,
                                    tile_info.clone(),
                                );
                            }
                        }
                    }

                    /*                     // need a list of tiles currently in the pipeline state, but not in the priority list
                    // these can be removed
                    let mut to_remove: Vec<u64> = Vec::new();
                    for tile_id in pipeline_state.iter() {
                        if !priority_list
                            .iter()
                            .any(|pri| pri.tile_content.id == *tile_id.0)
                        {
                            if let Err(_err) = send_unload_tile(*tile_id.0, pager_tx) {
                                // the channel is full, we will try again next time
                                break;
                            }

                            to_remove.push(*tile_id.0);
                        }
                    }
                    // remove the tiles from the pipeline state
                    pipeline_state.retain(|id, _count| !to_remove.contains(id)); */
                }
            }
        }
    }
    Ok(parsing_state)
}

pub async fn parser_thread(
    source: &Source,
    cam: Arc<Camera>,
    decoder_tx: &mut Sender<TilePipelineMessage>,
    renderer_tx: &mut Sender<TilePipelineMessage>,
    client: Client,
) -> Result<(), AbwError> {
    let mut root = None;
    let pipeline_state = TileManager::new();

    let mut last_cam_gen = 0;
    let mut parsing_gen = 1;
    let mut parsing_state = ParsingState::Instable;
    loop {
        let new_cam_gen = cam.generation();
        if new_cam_gen != last_cam_gen || parsing_state == ParsingState::Instable {
            let span = tracing::debug_span!("parser_iteration",).entered();

            let camera_data = cam.refinement_data();
            parsing_state = parser_iteration(
                source,
                &client,
                &camera_data,
                &mut root,
                &pipeline_state,
                decoder_tx,
                renderer_tx,
                parsing_gen,
            )?;

            last_cam_gen = new_cam_gen;
            parsing_gen += 1;

            drop(span);

            sleep_ms(10).await;
        } else {
            sleep_ms(250).await;
        }
    }

    Ok(())
}

pub fn build_client(threads: usize) -> Result<Client, AbwError> {
    Client::new(threads)
}
