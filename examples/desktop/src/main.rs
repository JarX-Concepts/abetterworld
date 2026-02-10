use tracing::{event, Level};
use wgpu_profiler::{GpuProfiler, GpuProfilerSettings};

use std::sync::Arc;
use winit::event::ElementState;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes},
};

const DEBUG_PATH: bool = false;
const DEBUG_CAMERA: bool = true;
const DEBUG_VOLUMES: bool = true;

// --- your existing imports ---
use abetterworld::{get_debug_config, InputEvent, Key, MouseButton, World};

// ---------------- State (unchanged, except where noted) ----------------

struct State<'window> {
    surface: wgpu::Surface<'window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    world: World,
    profiler: wgpu_profiler::GpuProfiler,
}

impl<'window> State<'window> {
    async fn new(window: &'window Window) -> Self {
        let size = window.inner_size();

        let desc = wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: Default::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        };
        let instance = wgpu::Instance::new(&desc);

        let surface = instance
            .create_surface(window)
            .expect("Failed to create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let needed_features = GpuProfiler::ALL_WGPU_TIMER_FEATURES;

        let adapter_features = adapter.features();

        // Only request features the adapter actually supports
        let required_features = needed_features & adapter_features;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features,
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
                label: None,
                trace: wgpu::Trace::Off,
            })
            .await
            .unwrap();

        let profiler = wgpu_profiler::GpuProfiler::new_with_tracy_client(
            GpuProfilerSettings::default(),
            adapter.get_info().backend,
            &device,
            &queue,
        )
        .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        let mut abw_config = get_debug_config();
        abw_config.use_debug_camera = DEBUG_CAMERA;
        abw_config.debug_render_volumes = DEBUG_VOLUMES;
        abw_config.debug_auto_tour = DEBUG_PATH;
        let world = World::new(
            &device,
            &config,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            &abw_config,
        );

        Self {
            surface,
            device,
            queue,
            config,
            size,
            world,
            profiler,
        }
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.size = new_size;
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
        self.world
            .resize(&self.device, new_size.width, new_size.height);
    }

    fn input(&mut self, event: InputEvent) {
        self.world.input(event);
    }

    fn update(&mut self) {
        let _ = self
            .world
            .update(&self.device, &self.queue)
            .map_err(|e| eprintln!("Update error: {e:?}"));
    }

    fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(self.world.get_depth_attachment()),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let _ = self.world.render(&mut rp);
        }

        self.profiler.resolve_queries(&mut encoder);

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

fn map_keycode(physical_key: &PhysicalKey) -> Option<Key> {
    match physical_key {
        PhysicalKey::Code(KeyCode::KeyW) => Some(Key::W),
        PhysicalKey::Code(KeyCode::KeyA) => Some(Key::A),
        PhysicalKey::Code(KeyCode::KeyS) => Some(Key::S),
        PhysicalKey::Code(KeyCode::KeyD) => Some(Key::D),

        PhysicalKey::Code(KeyCode::Equal | KeyCode::PageDown) => Some(Key::ZoomIn),
        PhysicalKey::Code(KeyCode::PageUp | KeyCode::Minus) => Some(Key::ZoomOut),

        PhysicalKey::Code(KeyCode::ArrowUp) => Some(Key::ArrowUp),
        PhysicalKey::Code(KeyCode::ArrowDown) => Some(Key::ArrowDown),
        PhysicalKey::Code(KeyCode::ArrowLeft) => Some(Key::ArrowLeft),
        PhysicalKey::Code(KeyCode::ArrowRight) => Some(Key::ArrowRight),
        PhysicalKey::Code(KeyCode::Escape) => Some(Key::Escape),
        PhysicalKey::Code(KeyCode::ShiftLeft | KeyCode::ShiftRight) => Some(Key::Shift),
        PhysicalKey::Code(KeyCode::ControlLeft | KeyCode::ControlRight) => Some(Key::Ctrl),
        PhysicalKey::Code(KeyCode::AltLeft | KeyCode::AltRight) => Some(Key::Alt),
        _ => None,
    }
}

struct App {
    window: Option<Arc<Window>>,
    state: Option<State<'static>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            window: None,
            state: None,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Replace WindowBuilder with default_attributes()
        let attrs: WindowAttributes = Window::default_attributes().with_title("A Better World");
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("create_window failed"),
        );
        let window_clone = Arc::clone(&window);

        let state = pollster::block_on(State::new(&window_clone));

        self.window = Some(window);

        #[allow(unsafe_code)]
        let state_static: State<'static> = unsafe { std::mem::transmute(state) };
        self.state = Some(state_static);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let (Some(window), Some(state)) = (&self.window, &mut self.state) else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(physical_size) => {
                state.resize(physical_size);
            }

            WindowEvent::ScaleFactorChanged { .. } => {
                state.resize(window.inner_size());
            }

            WindowEvent::KeyboardInput {
                event:
                    winit::event::KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => event_loop.exit(),

            WindowEvent::KeyboardInput {
                event:
                    winit::event::KeyEvent {
                        physical_key,
                        state: key_state,
                        ..
                    },
                ..
            } => {
                if let Some(key) = map_keycode(&physical_key) {
                    match key_state {
                        ElementState::Pressed => state.input(InputEvent::KeyPressed(key)),
                        ElementState::Released => state.input(InputEvent::KeyReleased(key)),
                    }
                }
            }

            WindowEvent::MouseInput {
                state: button_state,
                button,
                ..
            } => {
                let mapped = match button {
                    winit::event::MouseButton::Left => Some(MouseButton::Left),
                    winit::event::MouseButton::Right => Some(MouseButton::Right),
                    winit::event::MouseButton::Middle => Some(MouseButton::Middle),
                    _ => None,
                };
                if let Some(btn) = mapped {
                    match button_state {
                        ElementState::Pressed => state.input(InputEvent::MouseButtonPressed(btn)),
                        ElementState::Released => state.input(InputEvent::MouseButtonReleased(btn)),
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                state.input(InputEvent::MouseMoved(position.x as f64, position.y as f64));
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let scroll_delta = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y as f64,
                    winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y as f64,
                };
                state.input(InputEvent::MouseScrolled(scroll_delta));
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let (Some(window), Some(state)) = (&self.window, &mut self.state) else {
            return;
        };

        state.update();
        match state.render() {
            Ok(_) => {}
            Err(wgpu::SurfaceError::Lost) => state.resize(state.size),
            Err(wgpu::SurfaceError::OutOfMemory) => {
                // graceful shutdown when OOM
                #[cfg(not(target_arch = "wasm32"))]
                std::process::exit(0);
            }
            Err(e) => eprintln!("{e:?}"),
        }
        window.request_redraw();

        state.profiler.end_frame().unwrap();
    }
}

// ---------------- main ----------------

pub fn main() {
    dotenv::dotenv().ok();
    env_logger::init();
    event!(Level::INFO, "Starting A Better World application...");

    let event_loop = EventLoop::new().expect("Failed to create event loop");
    let mut app = App::default();
    event_loop.run_app(&mut app).expect("Event loop failed");
}
