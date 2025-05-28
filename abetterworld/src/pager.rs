use reqwest::Client;
use std::error::Error;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use threadpool::ThreadPool;
use tokio::runtime::Runtime;

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

pub fn start_update_in_range_loop(
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

                let in_range = import_tileset(&camera, &connection).await?;
                let content = tile_content.read().unwrap();
                let mut latest = content.latest_in_range.write().unwrap();
                *latest = in_range;
                latest.sort_by(|a, b| a.uri.cmp(&b.uri));
                latest.dedup_by(|a, b| a.uri == b.uri);
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
