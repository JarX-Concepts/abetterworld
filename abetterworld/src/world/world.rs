use crate::{
    cache::init_tileset_cache,
    content::{start_pager, Tile, TileManager},
    decode::init,
    dynamics::{init_camera, Camera, Dynamics, InputState},
    helpers::{
        channel::{channel, Receiver},
        AbwError,
    },
    render::{
        build_debug_pipeline, build_frustum_render, build_pipeline, DepthBuffer, FrustumRender,
        RenderAndUpdate, RenderPipeline,
    },
};
use std::{sync::Arc, time::Duration};

pub struct WorldPrivate {
    pub camera: Arc<Camera>,
    pub pipeline: RenderPipeline,
    pub depth: DepthBuffer,

    pub debug_camera: Arc<Camera>,
    pub debug_pipeline: RenderPipeline,
    pub frustum_render: FrustumRender,

    pub content: Arc<TileManager>,
    pub receiver: Receiver<Tile>,

    pub input_state: InputState,
    pub dynamics: Dynamics,
}

pub struct World {
    private: WorldPrivate,
    render: RenderAndUpdate,
}

pub enum Source {
    Google {
        key: String,
        url: String,
    },
    CesiumIon {
        key: String,
        url: String,
    },
    SelfHosted {
        headers: Vec<(String, String)>,
        url: String,
    },
}

pub struct Config {
    pub source: Source,
    pub start_position: (f64, f64, f64),
    pub cache_dir: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    W,
    A,
    S,
    D,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ZoomIn,
    ZoomOut,
    Shift,
    Ctrl,
    Alt,
    Escape,
    // Add more as needed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug)]
pub enum InputEvent {
    KeyPressed(Key),
    KeyReleased(Key),
    MouseMoved(f32, f32),
    MouseScrolled(f32),
    MouseButtonPressed(MouseButton),
    MouseButtonReleased(MouseButton),

    // Gesture input events
    GesturePinch {
        begin: bool,
        scale: f64,
        velocity: f64,
    },
    GestureOrbit {
        begin: bool,
        dx: f64,
        dy: f64,
        vx: f64,
        vy: f64,
    },
    GestureTranslate {
        begin: bool,
        dx: f64,
        dy: f64,
        vx: f64,
        vy: f64,
    },
    GestureRotate {
        begin: bool,
        radians: f64,
        velocity: f64,
    },
    GestureDoubleTap {
        x: f64,
        y: f64,
    },
    GestureTouchDown {
        active: bool,
        x: f64,
        y: f64,
    },
}

const MAX_NEW_TILES_PER_FRAME: usize = 4;

impl World {
    /// Creates a new ABetterWorld.
    pub fn new(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
        abw_config: &Config,
    ) -> Self {
        init_tileset_cache(&abw_config.cache_dir.to_string());

        let (camera, debug_camera) = init_camera();

        let pipeline = build_pipeline(device, config);
        let debug_pipeline = build_debug_pipeline(device, config);
        let depth = DepthBuffer::new(
            device,
            config.width,
            config.height,
            wgpu::TextureFormat::Depth24Plus,
            1,
        );
        let frustum_render = build_frustum_render(device);

        let tile_content = Arc::new(TileManager::new());
        let camera_source = Arc::new(camera);
        let debug_camera_source = Arc::new(debug_camera);

        camera_source.set_aspect(config.width as f64 / config.height as f64);
        debug_camera_source.set_aspect(config.width as f64 / config.height as f64);

        let (loader_tx, render_rx) = channel::<Tile>(MAX_NEW_TILES_PER_FRAME * 2);

        let _ = init();
        let _ = start_pager(debug_camera_source.clone(), tile_content.clone(), loader_tx);

        Self {
            private: WorldPrivate {
                dynamics: { Dynamics::new(camera_source.position()) },
                pipeline,
                debug_pipeline,
                camera: camera_source,
                debug_camera: debug_camera_source,
                depth,
                content: tile_content,
                input_state: InputState::new(),
                frustum_render,
                receiver: render_rx,
            },
            render: RenderAndUpdate::new(),
        }
    }

    pub fn get_depth_view(&self) -> &wgpu::TextureView {
        &self.private.depth.view
    }

    pub fn set_aspect(&self, aspect: f64) {
        self.private.camera.set_aspect(aspect);
    }

    pub fn resize(&mut self, device: &wgpu::Device, new_width: u32, new_height: u32) {
        if new_width == 0 || new_height == 0 {
            return;
        }

        // 2) Recreate depth buffer to match new size
        self.private.depth.resize(device, new_width, new_height);

        self.private
            .camera
            .set_aspect(new_width as f64 / new_height as f64);

        // 3) (If using MSAA) also recreate your MSAA color target here
    }

    pub fn render(
        &self,
        render_pass: &mut wgpu::RenderPass,
        queue: &wgpu::Queue,
        _device: &wgpu::Device,
    ) -> Result<(), AbwError> {
        self.render
            .render(render_pass, queue, &self.private, None, None)
    }

    pub fn update(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), AbwError> {
        self.private
            .dynamics
            .update(&core::time::Duration::from_millis(16), &self.private.camera);

        const BUDGET: Duration = Duration::from_millis(20);

        if let Some(layout) = self.private.pipeline.texture_bind_group_layout.as_ref() {
            self.private.content.unload_tiles();

            #[cfg(target_arch = "wasm32")]
            {
                let mut current_num_tiles = 0;

                // Pull tiles until either the channel is empty or we run out of time.
                while current_num_tiles < MAX_NEW_TILES_PER_FRAME {
                    current_num_tiles += 1;

                    match self.private.receiver.try_recv() {
                        Ok(mut tile) => {
                            use crate::content::tiles;
                            match tiles::content_render_setup(device, queue, layout, &mut tile) {
                                Ok(renderable_state) => {
                                    self.private.content.add_renderable(renderable_state);
                                }
                                Err(e) => {
                                    log::error!("Failed to set up tile for rendering: {e}");
                                    continue;
                                }
                            }
                        }
                        Err(_) => break, // nothing left
                    }
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                use std::time::Instant;

                let deadline = Instant::now() + BUDGET;
                // Pull tiles until either the channel is empty or we run out of time.
                while Instant::now() < deadline {
                    match self.private.receiver.try_recv() {
                        Ok(mut tile) => {
                            use crate::content::tiles;

                            match tiles::content_render_setup(device, queue, layout, &mut tile) {
                                Ok(renderable_state) => {
                                    self.private.content.add_renderable(renderable_state);
                                }
                                Err(e) => {
                                    log::error!("Failed to set up tile for rendering: {e}");
                                    continue;
                                }
                            }
                        }
                        Err(_) => break, // nothing left
                    }
                }
            }
        }

        //self.debug_camera.yaw(Deg(0.1));
        //self.debug_camera.write().unwrap().zoom(-500.0);
        self.private.debug_camera.update(None);
        let (eye_pos, uniform) = self.private.camera.update(None);

        self.render.update(
            device,
            queue,
            &mut self.private,
            &eye_pos,
            &uniform,
            None,
            None,
        )?;

        Ok(())
    }

    pub fn input(&mut self, event: InputEvent) {
        self.private
            .input_state
            .process_input(&self.private.dynamics, event);
    }
}
