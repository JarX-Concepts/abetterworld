use crate::{
    cache::init_tileset_cache,
    content::{import_renderables, start_pager, Tile, TileManager},
    decode::init,
    dynamics::{camera_config, Camera, Dynamics, InputState},
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

    pub debug_camera: Option<Arc<Camera>>,
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
    config: Config,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone)]
pub struct Config {
    pub source: Source,
    // long, lat, elevation
    pub geodetic_position: (f64, f64, f64),
    pub cache_dir: String,
    pub use_debug_camera: bool,
    pub debug_camera_geodetic_position: (f64, f64, f64),
    pub debug_camera_render_frustum: bool,
    pub debug_render_volumes: bool,
    pub tile_culling: bool,
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

pub const MAX_NEW_TILES_PER_FRAME: usize = 4;

impl World {
    /// Creates a new ABetterWorld.
    pub fn new(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
        abw_config: &Config,
    ) -> Self {
        init_tileset_cache(&abw_config.cache_dir.to_string());

        let (camera, debug_camera_option) = camera_config(abw_config);

        let pipeline = build_pipeline(device, config);
        let depth = DepthBuffer::new(
            device,
            config.width,
            config.height,
            wgpu::TextureFormat::Depth24Plus,
            1,
        );

        let debug_pipeline = build_debug_pipeline(device, config);
        let frustum_render = build_frustum_render(device);

        let tile_content = Arc::new(TileManager::new());

        camera.set_aspect(config.width as f64 / config.height as f64);

        let (loader_tx, render_rx) = channel::<Tile>(MAX_NEW_TILES_PER_FRAME * 2);

        let _ = init();
        let _ = start_pager(
            abw_config.source.clone(),
            Arc::clone(debug_camera_option.as_ref().unwrap_or(&camera)),
            tile_content.clone(),
            loader_tx,
        );

        Self {
            private: WorldPrivate {
                dynamics: { Dynamics::new(camera.position()) },
                pipeline,
                debug_pipeline,
                camera: camera,
                debug_camera: debug_camera_option,
                depth,
                content: tile_content,
                input_state: InputState::new(),
                frustum_render,
                receiver: render_rx,
            },
            render: RenderAndUpdate::new(),
            config: abw_config.clone(),
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

    pub fn render(&self, render_pass: &mut wgpu::RenderPass) -> Result<(), AbwError> {
        self.render.render(
            render_pass,
            &self.private,
            self.config.debug_render_volumes,
            self.config.use_debug_camera,
        )
    }

    pub fn update(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<bool, AbwError> {
        self.private
            .dynamics
            .update(&core::time::Duration::from_millis(16), &self.private.camera);

        const BUDGET: Duration = Duration::from_millis(20);

        let mut needs_update = false;

        if let Some(layout) = self.private.pipeline.texture_bind_group_layout.as_ref() {
            needs_update = self.private.content.unload_tiles();

            needs_update |= import_renderables(
                device,
                queue,
                layout,
                &self.private.content,
                &mut self.private.receiver,
                BUDGET,
            )?;
        }

        // Update the debug camera if it exists
        if let Some(debug_camera) = self.private.debug_camera.as_ref() {
            let min_distance = self.render.get_min_distance(&debug_camera.position().eye);

            let (_, _, dirty) = debug_camera.update(min_distance);
            if dirty {
                needs_update = true;
            }
        }
        let min_distance = self
            .render
            .get_min_distance(&self.private.camera.position().eye);
        let (eye_pos, uniform, dirty) = self.private.camera.update(min_distance);
        if dirty {
            needs_update = true;
        }

        // we do not need to update anything
        if needs_update {
            self.render.update(
                device,
                queue,
                &mut self.private,
                &eye_pos,
                &uniform,
                self.config.debug_render_volumes,
                self.config.use_debug_camera,
                self.config.tile_culling,
            )?;
        }

        Ok(needs_update)
    }

    pub fn input(&mut self, event: InputEvent) {
        self.private
            .input_state
            .process_input(&self.private.dynamics, event);
    }
}
