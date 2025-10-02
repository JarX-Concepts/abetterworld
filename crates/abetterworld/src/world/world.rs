use cgmath::{Point3, Vector3};
//use tracing::instrument;

use crate::{
    cache::init_tileset_cache,
    content::{import_renderables, start_pager, Tile, TileManager},
    dynamics::{self, camera_config, Camera, Dynamics, InputState, PositionState},
    helpers::{
        channel::{channel, Receiver},
        geodetic_to_ecef_z_up, hpr_to_forward_up, init_profiling, target_from_distance, AbwError,
        FrameClock,
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

    pub debug_camera: Option<Arc<Camera>>,
    pub debug_pipeline: RenderPipeline,
    pub frustum_render: FrustumRender,

    pub content: Arc<TileManager>,
    pub receiver: Receiver<Tile>,

    pub input_state: InputState,
    pub dynamics: Dynamics,

    pub clock: FrameClock,
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
    Meta,
    // Add more as needed
    Count,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Count = 3,
}

#[derive(Debug)]
pub enum InputEvent {
    KeyPressed(Key),
    KeyReleased(Key),
    MouseMoved(f64, f64),
    MouseScrolled(f64),
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
    GestureTouchMove {
        id: u64,
        x: f64,
        y: f64,
    },
    GestureTouchUp {
        id: u64,
    },
    GestureTap {
        x: f64,
        y: f64,
    },

    WindowFocused(bool),
    PointerCapture(bool),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Location {
    Geodetic(f64, f64, f64),
    Geocentric(f64, f64, f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Orientation {
    HeadingPitchRoll(f64, f64, f64),
    TargetUp((f64, f64, f64), (f64, f64, f64)),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPosition {
    pub location: Location,
    pub orientation: Orientation,
}

// backpressure limit on new tiles per frame
pub const MAX_NEW_TILES_PER_FRAME: usize = 4;

impl World {
    /// Creates a new ABetterWorld.
    pub fn new(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
        abw_config: &Config,
    ) -> Self {
        init_profiling();

        init_tileset_cache(&abw_config.cache_dir.to_string());

        let (camera, debug_camera_option) = camera_config(abw_config);

        let pipeline = build_pipeline(device, config);

        let debug_pipeline =
            build_debug_pipeline(device, config, &pipeline.depth.as_ref().unwrap());
        let frustum_render = build_frustum_render(device);

        let tile_content = Arc::new(TileManager::new());

        camera.set_viewport(config.width as f64, config.height as f64);

        let (loader_tx, render_rx) = channel::<Tile>(MAX_NEW_TILES_PER_FRAME * 2);

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
                content: tile_content,
                input_state: InputState::new(),
                frustum_render,
                receiver: render_rx,
                clock: FrameClock::new(std::time::Duration::from_millis(16), 0.2),
            },
            render: RenderAndUpdate::new(),
            config: abw_config.clone(),
        }
    }

    pub fn get_depth_view(&self) -> &wgpu::TextureView {
        &self.private.pipeline.depth.as_ref().unwrap().view
    }

    pub fn get_depth_attachment(&self) -> wgpu::RenderPassDepthStencilAttachment {
        self.private
            .pipeline
            .depth
            .as_ref()
            .unwrap()
            .attachment_clear()
    }

    pub fn resize(&mut self, device: &wgpu::Device, new_width: u32, new_height: u32) {
        if new_width == 0 || new_height == 0 {
            return;
        }

        // 2) Recreate depth buffer to match new size
        self.private
            .pipeline
            .depth
            .as_mut()
            .unwrap()
            .resize(device, new_width, new_height);

        self.private
            .camera
            .set_viewport(new_width as f64, new_height as f64);

        // 3) (If using MSAA) also recreate the MSAA color target here
    }

    //#[instrument(skip(self, render_pass))]
    pub fn render(&self, render_pass: &mut wgpu::RenderPass) -> Result<(), AbwError> {
        self.render.render(
            render_pass,
            &self.private,
            self.config.debug_render_volumes,
            self.config.use_debug_camera,
        )
    }

    //#[instrument(skip(self, device, queue), fields(need_update = false))]
    pub fn update(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<bool, AbwError> {
        let tick = self.private.clock.tick();

        //log::info!("Frame tick: {:?}", tick);

        self.private.input_state.flush(&mut self.private.dynamics);
        self.private
            .dynamics
            .update(&tick.elapsed, &self.private.camera);

        const BUDGET: Duration = Duration::from_millis(16);

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

            let (_, _, dirty) = debug_camera.update();
            if dirty {
                needs_update = true;
            }
        }
        let min_distance = self
            .render
            .get_min_distance(&self.private.camera.position().eye);
        let (eye_pos, uniform, dirty) = self.private.camera.update();
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
        self.private.input_state.queue_event(
            &self.private.camera.dynamics(),
            &mut self.private.dynamics,
            event,
            &self.private.camera,
        );
    }

    pub fn set_camera_position(&self, position: CameraPosition, debug_camera: bool) {
        let camera = if debug_camera {
            self.private.debug_camera.as_ref()
        } else {
            Some(&self.private.camera)
        };

        if let Some(cam) = camera {
            let loc = match position.location {
                Location::Geodetic(lon, lat, alt) => geodetic_to_ecef_z_up(lon, lat, alt),
                Location::Geocentric(x, y, z) => Point3::new(x, y, z),
            };
            let (target, up) = match position.orientation {
                Orientation::HeadingPitchRoll(h, p, r) => {
                    let (f, u) = hpr_to_forward_up(h, p, r);
                    (target_from_distance(loc, &f, 1.0), u)
                }
                Orientation::TargetUp((tx, ty, tz), (ux, uy, uz)) => {
                    (Point3::new(tx, ty, tz), Vector3::new(ux, uy, uz))
                }
            };
            cam.set_position(&PositionState {
                eye: loc,
                target,
                up,
            });

            if !debug_camera {
                self.private.dynamics.set_position(&PositionState {
                    eye: loc,
                    target,
                    up,
                });
            }
        }
    }
}
