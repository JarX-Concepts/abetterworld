use reqwest::Client;
use std::error::Error;
use std::sync::{Arc, RwLock};
use threadpool::ThreadPool;
use tokio::runtime::Runtime;

#[cfg(not(target_arch = "wasm32"))]
use std::thread;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures;

use crate::camera::Camera;
use crate::content::{ContentInRange, ContentLoaded, ContentRender};
use crate::tiles::{
    content_load, content_render, import_tileset, load_root, Connection, ConnectionState,
};

pub struct TileContent {
    latest_in_range: Arc<RwLock<Vec<ContentInRange>>>,
    latest_loaded: Arc<RwLock<Vec<ContentLoaded>>>,
    pub latest_render: Vec<ContentRender>, // still on main thread
}

pub async fn establish_connection(client: Arc<Client>) -> Result<ConnectionState, Box<dyn Error>> {
    let (url, key) = load_root(&client).await?;
    let connection = ConnectionState {
        connection: Connection {
            client: client.clone(),
            key,
        },
        tileset_url: url,
        tile: None,
        session: None,
    };

    return Ok(connection);
}

pub async fn run_update_in_range_once(
    tile_content: Arc<RwLock<TileContent>>,
    camera: Camera,
    connection: &ConnectionState,
) -> Result<(), Box<dyn Error>> {
    let mut in_range = import_tileset(&camera, &connection).await?;
    in_range.sort_by(|a, b| a.uri.cmp(&b.uri));
    in_range.dedup_by(|a, b| a.uri == b.uri);

    // Scope to ensure locks are dropped
    let should_update = {
        let content = tile_content.read().unwrap();
        let latest = content.latest_in_range.read().unwrap();
        *latest != in_range
    };

    if should_update {
        let content = tile_content.read().unwrap();
        let mut latest_mut = content.latest_in_range.write().unwrap();
        *latest_mut = in_range;
    }
    Ok::<(), Box<dyn Error>>(())
}

pub async fn content_decode(
    job_conn: &Connection,
    loaded: Arc<RwLock<Vec<ContentLoaded>>>,
    tile: &ContentInRange,
) -> Result<(), Box<dyn Error>> {
    if let Ok(loaded_tile) = content_load(&job_conn.client, job_conn.key.as_str(), &tile).await {
        loaded.write().unwrap().push(loaded_tile);
        Ok(())
    } else {
        log::error!("Failed to load content for tile: {}", tile.uri);
        Err("Failed to load content".into())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn run_load_worker_once(
    tile_content: Arc<RwLock<TileContent>>,
    key: &str,
    pool: ThreadPool,
) {
    let key_owned = key.to_string();
    let (in_range, loaded_arc) = {
        let tc = tile_content.read().unwrap();
        let in_range = {
            let latest_in_range = tc.latest_in_range.read().unwrap();
            latest_in_range.clone()
        };
        let loaded_arc = tc.latest_loaded.clone();
        (in_range, loaded_arc)
    };

    for tile in in_range {
        let loaded = loaded_arc.clone();
        let key = key_owned.clone();

        // Skip if already loaded
        if loaded.read().unwrap().iter().any(|l| l.uri == tile.uri) {
            continue;
        }

        pool.execute(move || {
            // Use a runtime per worker thread
            let local_rt = Runtime::new().expect("Worker thread failed to create Tokio runtime");

            // Create a client inside this runtime
            let client = Arc::new(Client::new());
            let local_job_conn = Connection { client, key };

            let fut = content_decode(&local_job_conn, loaded.clone(), &tile);

            if let Err(e) = local_rt.block_on(fut) {
                log::error!(
                    "Failed to decode content for tile {}: {:?}",
                    tile.uri.clone(),
                    e
                );
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
pub async fn run_load_worker_once(tile_content: Arc<RwLock<TileContent>>, connection: &Connection) {
    let (in_range, loaded_arc) = {
        let tc = tile_content.read().unwrap();
        let in_range = tc.latest_in_range.read().unwrap().clone();
        let loaded_arc = tc.latest_loaded.clone();
        (in_range, loaded_arc)
    };

    for tile in in_range {
        let loaded = loaded_arc.clone();

        if loaded.read().unwrap().iter().any(|l| l.uri == tile.uri) {
            continue;
        }

        let local_connection = connection.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = content_decode(&local_connection, loaded, &tile).await;
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn start_background_tasks(
    tile_content: Arc<RwLock<TileContent>>,
    camera_source: Arc<RwLock<Camera>>,
) -> Result<(), Box<dyn Error>> {
    log::debug!("Establishing Connection");

    let client = Arc::new(Client::new());

    let rt = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime"),
    );

    let connection_future = establish_connection(client.clone());
    let connection = rt.block_on(connection_future)?;
    let key = connection.connection.key.clone();

    // Update in range thread
    std::thread::spawn({
        let tile_content = tile_content.clone();
        move || {
            let rt = tokio::runtime::Runtime::new().unwrap();

            let client = Arc::new(Client::new());
            let local_job_conn = ConnectionState {
                connection: Connection {
                    client: client.clone(),
                    key: connection.connection.key.clone(),
                },
                tileset_url: connection.tileset_url.clone(),
                tile: None,
                session: None,
            };

            loop {
                let camera = camera_source.read().unwrap().clone();
                if let Err(e) = rt.block_on(run_update_in_range_once(
                    tile_content.clone(),
                    camera,
                    &local_job_conn,
                )) {
                    eprintln!("update_in_range error: {:?}", e);
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    });

    // Load worker thread
    std::thread::spawn({
        move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            let pool = ThreadPool::new(8);
            loop {
                rt.block_on(run_load_worker_once(
                    tile_content.clone(),
                    key.as_str(),
                    pool.clone(),
                ));
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    });

    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn start_background_tasks(
    tile_content: Arc<RwLock<TileContent>>,
    camera_source: Arc<RwLock<Camera>>,
) -> Result<(), Box<dyn Error>> {
    use gloo_timers::future;
    use wasm_bindgen_futures::spawn_local;

    use crate::camera;

    spawn_local(async move {
        log::info!("Establish Connection");
        let client = Arc::new(Client::new());
        let connection_result = establish_connection(client.clone()).await;
        let connection = match connection_result {
            Ok(conn) => conn,
            Err(e) => {
                log::error!("Failed to establish connection: {:?}", e);
                return;
            }
        };

        {
            let tile_content = tile_content.clone();
            let connection = connection.clone();
            let camera_source = camera_source.clone();
            spawn_local(async move {
                loop {
                    let camera = camera_source.read().unwrap().clone();
                    let tile_content = tile_content.clone();
                    let connection = connection.clone();

                    if let Err(e) =
                        run_update_in_range_once(tile_content, camera, &connection).await
                    {
                        log::error!("update_in_range error: {:?}", e);
                    }

                    // Wait 250ms after the task finishes
                    gloo_timers::future::sleep(std::time::Duration::from_millis(250)).await;
                }
            });
        }

        {
            let tile_content = tile_content.clone();
            let connection = connection.clone();
            spawn_local(async move {
                loop {
                    let camera = camera_source.read().unwrap().clone();
                    let tile_content = tile_content.clone();
                    let connection = connection.clone();

                    run_load_worker_once(tile_content, &connection.connection).await;

                    // Wait 250ms after the task finishes
                    gloo_timers::future::sleep(std::time::Duration::from_millis(250)).await;
                }
            });
        }
    });

    Ok(())
}

/* pub fn start_update_in_range_loop(
    tile_content: Arc<RwLock<TileContent>>,
    camera_source: Arc<RwLock<Camera>>,
) {
    let rt = Arc::new(
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime"),
    );

    let client = Arc::new(Client::new());

    thread::spawn({
        let rt = rt.clone();
        let client = client.clone();
        move || loop {
            let camera = camera_source.read().unwrap().clone();
            let tile_content = tile_content.clone(); // clone Arc to move inside

            let fut = async {
                let (url, key) = load_root(&client).await?;
                let connection = ConnectionState {
                    connection: Connection {
                        client: client.clone(),
                        key,
                    },
                    tileset_url: url,
                    tile: None,
                    session: None,
                };

                let mut in_range = import_tileset(&camera, &connection).await?;
                in_range.sort_by(|a, b| a.uri.cmp(&b.uri));
                in_range.dedup_by(|a, b| a.uri == b.uri);

                let content = tile_content.read().unwrap();

                // Scope to ensure locks are dropped
                let should_update = {
                    let content = tile_content.read().unwrap();
                    let latest = content.latest_in_range.read().unwrap();
                    *latest != in_range
                };

                if should_update {
                    let content = tile_content.read().unwrap();
                    let mut latest_mut = content.latest_in_range.write().unwrap();
                    *latest_mut = in_range;
                }
                Ok::<(), Box<dyn Error>>(())
            };

            if let Err(e) = rt.block_on(fut) {
                eprintln!("update_in_range_loop error: {:?}", e);
            }

            thread::sleep(Duration::from_millis(250));
        }
    });
}

pub fn start_load_worker_pool(tile_content: Arc<RwLock<TileContent>>, pool: ThreadPool) {
    let client = Arc::new(Client::new());

    thread::spawn({
        move || {
            let _rt = Runtime::new().expect("Failed to create Tokio runtime");

            loop {
                let (in_range, loaded_arc) = {
                    let tc = tile_content.read().unwrap();
                    let in_range = {
                        let latest_in_range = tc.latest_in_range.read().unwrap();
                        latest_in_range.clone()
                    };
                    let loaded_arc = tc.latest_loaded.clone();
                    (in_range, loaded_arc)
                };

                for tile in in_range {
                    let loaded = loaded_arc.clone();

                    // Skip if already loaded
                    if loaded.read().unwrap().iter().any(|l| l.uri == tile.uri) {
                        continue;
                    }

                    let job_client = client.clone();
                    pool.execute(move || {
                        let fut = async move {
                            match content_load(&job_client, tile.session.as_str(), &tile).await {
                                Ok(loaded_tile) => {
                                    loaded.write().unwrap().push(loaded_tile);
                                }
                                Err(e) => {
                                    eprintln!("load_worker error: {:?}", e);
                                }
                            }
                        };

                        // Use a runtime per worker thread
                        let local_rt =
                            Runtime::new().expect("Worker thread failed to create Tokio runtime");
                        local_rt.block_on(fut);
                    });
                }

                thread::sleep(Duration::from_millis(100));
            }
        }
    });
}
 */
impl TileContent {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            latest_in_range: Arc::new(RwLock::new(Vec::new())),
            latest_loaded: Arc::new(RwLock::new(Vec::new())),
            latest_render: Vec::new(),
        })
    }

    pub fn update_render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture_bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Result<(), Box<dyn Error>> {
        let loaded = self.latest_loaded.read().unwrap();
        for l in loaded.iter() {
            if self.latest_render.iter().any(|r| r.uri == l.uri) {
                continue;
            }
            let render = content_render(device, queue, texture_bind_group_layout, l)?;
            self.latest_render.push(render);
        }

        self.latest_render
            .retain(|r| loaded.iter().any(|l| l.uri == r.uri));

        Ok(())
    }
}
