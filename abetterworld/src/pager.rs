// Optimized Tile Content System for Rust + WASM
use reqwest::Client;
use std::error::Error;
use std::sync::{Arc, RwLock};
use threadpool::ThreadPool;
use tokio::runtime::Builder;
use tokio::runtime::Runtime;

use std::collections::HashSet;
use tokio::sync::{Mutex, Semaphore};

#[cfg(not(target_arch = "wasm32"))]
use std::thread;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures;

#[cfg(target_arch = "wasm32")]
use gloo_timers::future::TimeoutFuture;

#[cfg(not(target_arch = "wasm32"))]
use tokio::time::{sleep, Duration};

async fn wait_short_delay() {
    #[cfg(target_arch = "wasm32")]
    TimeoutFuture::new(10).await;

    #[cfg(not(target_arch = "wasm32"))]
    sleep(Duration::from_millis(10)).await;
}

async fn wait_longer_delay() {
    #[cfg(target_arch = "wasm32")]
    TimeoutFuture::new(1000).await;

    #[cfg(not(target_arch = "wasm32"))]
    sleep(Duration::from_millis(1000)).await;
}

use crate::camera::Camera;
use crate::content::{ContentInRange, ContentLoaded, ContentRender};
use crate::tiles::{
    content_render, download_content_for_tile, import_tileset, load_root, process_content_bytes,
    Connection, ConnectionState,
};

pub struct TileContent {
    latest_in_range: Arc<tokio::sync::RwLock<Vec<ContentInRange>>>,
    latest_loaded: Arc<tokio::sync::RwLock<Vec<ContentLoaded>>>,
    pub latest_render: Arc<RwLock<Vec<ContentRender>>>, // Sync access for render thread
}

impl TileContent {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            latest_in_range: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            latest_loaded: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            latest_render: Arc::new(RwLock::new(Vec::new())),
        })
    }

    pub fn add_in_range(&self, item: &ContentInRange) {
        let item = item.clone();
        let lock = self.latest_in_range.clone();

        #[cfg(not(target_arch = "wasm32"))]
        {
            tokio::spawn(async move {
                if lock.read().await.iter().any(|x| x.uri == item.uri) {
                    return;
                }
                let mut vec = lock.write().await;
                vec.push(item);
            });
        }

        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen_futures::spawn_local;

            spawn_local(async move {
                if lock.read().await.iter().any(|x| x.uri == item.uri) {
                    return;
                }
                let mut vec = lock.write().await;
                vec.push(item);

                log::info!("Added tile to in-range");
            });
        }
    }

    pub fn update_render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
    ) -> Result<(), Box<dyn Error>> {
        let loaded = self.latest_loaded.blocking_read();
        let mut render = self.latest_render.write().unwrap();

        for item in loaded.iter() {
            if !render.iter().any(|r| r.uri == item.uri) {
                let new_render = content_render(device, queue, layout, item)?;
                render.push(new_render);
            }
        }

        render.retain(|r| loaded.iter().any(|l| l.uri == r.uri));
        Ok(())
    }
}

pub async fn establish_connection(client: Arc<Client>) -> Result<ConnectionState, Box<dyn Error>> {
    let (url, key) = load_root(&client).await?;
    Ok(ConnectionState {
        connection: Connection {
            client,
            key: key.clone(),
        },
        tileset_url: url,
        tile: None,
        session: None,
    })
}

pub async fn run_update_in_range_once(
    tile_content: Arc<TileContent>,
    camera: Camera,
    connection: &ConnectionState,
) -> Result<(), Box<dyn Error>> {
    let tile_content_clone = tile_content.clone();
    let add_tile = Arc::new(move |tile: &ContentInRange| {
        tile_content_clone.add_in_range(tile);
    });

    import_tileset(&camera, connection, add_tile).await?;

    /*
    in_range.sort_by(|a, b| a.uri.cmp(&b.uri));
    in_range.dedup_by(|a, b| a.uri == b.uri);

    let should_update = {
        let current = tile_content.latest_in_range.read().await;
        *current != in_range
    };

    if should_update {
        let mut current = tile_content.latest_in_range.write().await;
        *current = in_range;
    } */

    Ok(())
}

pub async fn content_load(
    client: &Client,
    key: &str,
    load: &ContentInRange,
) -> Result<ContentLoaded, Box<dyn Error + Send + Sync>> {
    let (bytes, _content_type) = download_content_for_tile(client, key, load).await.unwrap();
    Ok(process_content_bytes(&load, &load.session, bytes).unwrap())
}

pub async fn content_decode(
    job_conn: &Connection,
    loaded: Arc<tokio::sync::RwLock<Vec<ContentLoaded>>>,
    tile: &ContentInRange,
) -> Result<(), Box<dyn Error>> {
    match content_load(&job_conn.client, &job_conn.key, tile).await {
        Ok(loaded_tile) => {
            let mut guard = loaded.write().await;
            guard.push(loaded_tile);
            Ok(())
        }
        Err(e) => {
            log::error!("Failed to load content for tile {}: {:?}", tile.uri, e);
            Err("Failed to load content".into())
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn start_background_tasks(
    tile_content: Arc<TileContent>,
    camera_source: Arc<RwLock<Camera>>,
) -> Result<(), Box<dyn Error>> {
    let client = Arc::new(Client::new());
    let connection = establish_connection(client.clone()).await?;
    let key = connection.connection.key.clone();

    {
        let tile_content = tile_content.clone();
        let camera_source = camera_source.clone();
        let tileset_url = connection.tileset_url.clone();
        thread::spawn(move || {
            let rt = Runtime::new().unwrap();
            let local_conn = ConnectionState {
                connection: Connection {
                    client: Arc::new(Client::new()),
                    key: key.clone(),
                },
                tileset_url,
                tile: None,
                session: None,
            };

            loop {
                //log::info!("Running update_in_range");
                let camera = camera_source.read().unwrap().clone();
                if let Err(e) = rt.block_on(run_update_in_range_once(
                    tile_content.clone(),
                    camera,
                    &local_conn,
                )) {
                    log::error!("update_in_range error: {:?}", e);
                }
                //log::info!("Done update_in_range");
                thread::sleep(std::time::Duration::from_millis(30));
            }
        });
    }

    {
        let tile_content = tile_content.clone();
        let key = connection.connection.key.clone();
        thread::spawn(move || {
            let rt = Arc::new(Builder::new_current_thread().enable_all().build().unwrap());

            let client = Arc::new(Client::new());
            let max_threads = 20;
            let processing_list = Arc::new(Mutex::new(HashSet::new()));
            let pool = ThreadPool::new(max_threads);

            loop {
                let (tiles, loaded) = rt.block_on(async {
                    let tc = &tile_content;
                    let tiles = tc.latest_in_range.read().await.clone();
                    let loaded = tc.latest_loaded.clone();
                    (tiles, loaded)
                });

                let loaded_vec = rt.block_on(async { loaded.read().await.clone() });

                for tile in tiles {
                    if loaded_vec.iter().any(|l| l.uri == tile.uri) {
                        continue;
                    }

                    // Check + mark as processing
                    let mut list = rt.block_on(async { processing_list.lock().await });
                    if !list.insert(tile.uri.clone()) {
                        continue; // already processing
                    }
                    drop(list); // release lock early

                    // do not overwhelm the thread pool
                    while pool.queued_count() >= max_threads {
                        rt.block_on(wait_short_delay());
                    }

                    let loaded = loaded.clone();
                    let key = key.clone();
                    let client = client.clone();
                    let rt = rt.clone();
                    let uri: String = tile.uri.clone();
                    let processing_list = processing_list.clone();

                    pool.execute(move || {
                        let conn = Connection { client, key };
                        let fut = async move {
                            if let Err(e) = content_decode(&conn, loaded, &tile).await {
                                log::error!("content_decode error for {}: {:?}", tile.uri, e);
                            }
                        };

                        let _ = rt.block_on(fut);

                        // Remove from processing list
                        let mut list = rt.block_on(async { processing_list.lock().await });
                        list.remove(&uri);
                    });
                }

                thread::sleep(std::time::Duration::from_millis(3));
            }
        });
    }

    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub async fn start_background_tasks(
    tile_content: Arc<TileContent>,
    camera_source: Arc<RwLock<Camera>>,
) -> Result<(), Box<dyn Error>> {
    use futures_util::StreamExt;
    use gloo_timers::future::IntervalStream;
    use wasm_bindgen_futures::spawn_local;

    let client = Arc::new(Client::new());
    let connection = establish_connection(client.clone()).await?;
    let key = connection.connection.key.clone();
    let tileset_url = connection.tileset_url.clone();

    // Continuous update_in_range task (runs in the background)
    {
        let tile_content = tile_content.clone();
        let camera_source = camera_source.clone();
        let local_conn = ConnectionState {
            connection: Connection {
                client: client.clone(),
                key: key.clone(),
            },
            tileset_url: tileset_url.clone(),
            tile: None,
            session: None,
        };

        spawn_local(async move {
            loop {
                let camera = camera_source.read().unwrap().clone();

                if let Err(e) =
                    run_update_in_range_once(tile_content.clone(), camera, &local_conn).await
                {
                    log::error!("update_in_range error: {:?}", e);
                }

                wait_longer_delay().await;
            }
        });
    }

    // Continuous decode task (fully async)
    {
        let tile_content = tile_content.clone();
        let key = key.clone();
        let client = client.clone();

        let max_concurrent_decodes = 10;
        let processing_list = Arc::new(Mutex::new(HashSet::new()));
        let semaphore = Arc::new(Semaphore::new(max_concurrent_decodes)); // already in your code

        spawn_local(async move {
            loop {
                use std::{thread::sleep, time::Duration};

                let tiles;
                let loaded;

                {
                    let current = tile_content.latest_in_range.read().await;
                    tiles = current.clone();
                    loaded = tile_content.latest_loaded.clone();
                }

                let loaded_vec = loaded.read().await.clone();

                for tile in tiles {
                    if loaded_vec.iter().any(|l| l.uri == tile.uri) {
                        continue;
                    }

                    // Check + mark as processing
                    let mut list = processing_list.lock().await;
                    if !list.insert(tile.uri.clone()) {
                        continue; // already processing
                    }
                    drop(list); // release lock early

                    let permit = match semaphore.clone().try_acquire_owned() {
                        Ok(p) => p,
                        Err(_) => {
                            // Rollback processing mark if skipping
                            let mut list = processing_list.lock().await;
                            list.remove(&tile.uri);
                            break;
                        }
                    };

                    let loaded = loaded.clone();
                    let key = key.clone();
                    let client = client.clone();
                    let uri = tile.uri.clone();
                    let processing_list = processing_list.clone();

                    spawn_local(async move {
                        let conn = Connection { client, key };
                        if let Err(e) = content_decode(&conn, loaded, &tile).await {
                            log::error!("content_decode error for {}: {:?}", uri, e);
                        }

                        // Remove from processing list
                        let mut list = processing_list.lock().await;
                        list.remove(&uri);
                        drop(permit);
                    });
                }

                wait_longer_delay().await;
            }
        });
    }

    Ok(())
}
