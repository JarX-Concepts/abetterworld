use abetterworld::Key;
use abetterworld::{get_debug_config, InputEvent, MouseButton, World};
use std::rc::Rc;
use std::sync::Arc;
use tracing::{event, Level};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use winit::event::ElementState;
use winit::platform::web::WindowExtWebSys;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowAttributes},
};

fn setup_console_log() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Redirect `println!` and `eprintln!`
        console_log::init_with_level(log::Level::Trace).expect("error initializing log");
        // Also show panic messages in console
        console_error_panic_hook::set_once();

        event!(Level::INFO, "Console logging initialized");
    });
}

fn logical_window_size() -> (u32, u32) {
    let win = web_sys::window().unwrap();
    let w = win
        .inner_width()
        .unwrap()
        .as_f64()
        .unwrap()
        .round()
        .max(1.0) as u32;
    let h = win
        .inner_height()
        .unwrap()
        .as_f64()
        .unwrap()
        .round()
        .max(1.0) as u32;
    (w, h)
}

fn device_pixel_ratio() -> f64 {
    web_sys::window().unwrap().device_pixel_ratio()
}

fn size_canvas_backing_store(canvas: &web_sys::HtmlCanvasElement) -> winit::dpi::PhysicalSize<u32> {
    let (lw, lh) = logical_window_size();
    let dpr = device_pixel_ratio();
    let pw = ((lw as f64) * dpr).round().max(1.0) as u32;
    let ph = ((lh as f64) * dpr).round().max(1.0) as u32;
    canvas.set_width(pw);
    canvas.set_height(ph);
    winit::dpi::PhysicalSize::new(pw, ph)
}

struct State<'window> {
    surface: wgpu::Surface<'window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    world: World,
}

impl<'window> State<'window> {
    async fn new(
        window: &'window winit::window::Window,
        canvas: &web_sys::HtmlCanvasElement,
    ) -> Self {
        // Prefer the canvas’ backing size; winit’s inner_size may be 0 on web.
        let mut size = winit::dpi::PhysicalSize::new(canvas.width(), canvas.height());
        if size.width == 0 || size.height == 0 {
            size = winit::dpi::PhysicalSize::new(800, 600);
            web_sys::console::log_1(&"Using default size (800x600) in State::new".into());
        }

        let desc = wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: Default::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        };
        let instance = wgpu::Instance::new(&desc);
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .expect("Failed to create surface from canvas"); // <-- no `unsafe`, no from_canvas()

        // Request adapter/device as before
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                label: None,
                memory_hints: Default::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .unwrap();

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, wgpu::TextureFormat::Rgba8Unorm))
            .unwrap_or(surface_caps.formats[0]);

        tracing::info!(
            "Surface format selected: {:?}, available: {:?}",
            surface_format,
            surface_caps.formats
        );

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

        let world = World::new(&device, &config, surface_format, &get_debug_config());

        Self {
            surface,
            device,
            queue,
            config,
            size,
            world,
        }
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        web_sys::console::log_1(
            &format!("Resizing to: {}x{}", new_size.width, new_size.height).into(),
        );

        // Ensure width and height are never zero
        let width = new_size.width.max(1);
        let height = new_size.height.max(1);

        self.size = winit::dpi::PhysicalSize::new(width, height);
        self.config.width = width;
        self.config.height = height;

        web_sys::console::log_1(
            &format!(
                "Config updated to: {}x{}",
                self.config.width, self.config.height
            )
            .into(),
        );

        self.surface.configure(&self.device, &self.config);
        self.world
            .resize(&self.device, self.config.width, self.config.height);
    }

    fn input(&mut self, event: InputEvent) {
        // No dynamic updates for now.
        self.world.input(event);
    }

    fn update(&mut self) {
        self.world.update(&mut &self.device, &self.queue);
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
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.01,
                            g: 0.01,
                            b: 0.01,
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
            self.world.render(&mut render_pass);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

struct WebApp {
    window: Option<Arc<Window>>,
    canvas: Option<web_sys::HtmlCanvasElement>,
    state: std::rc::Rc<std::cell::RefCell<Option<State<'static>>>>,
}

impl Default for WebApp {
    fn default() -> Self {
        Self {
            window: None,
            canvas: None,
            state: std::rc::Rc::new(std::cell::RefCell::new(None)),
        }
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

impl ApplicationHandler for WebApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Create window
        let attrs: WindowAttributes = Window::default_attributes()
            .with_title("A Better World WASM")
            .with_inner_size(winit::dpi::PhysicalSize::new(800, 600));

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("create_window failed"),
        );

        // Extract canvas and attach to document
        let canvas = window.canvas().expect("Failed to get canvas");
        canvas.set_id("canvas");
        let document = web_sys::window().unwrap().document().unwrap();
        document.body().unwrap().append_child(&canvas).unwrap();

        let canvas: web_sys::HtmlCanvasElement = document
            .get_element_by_id("canvas")
            .unwrap()
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .unwrap();
        canvas.style().set_property("display", "block").unwrap();
        canvas.style().set_property("margin", "0").unwrap();

        // Backing store size (accounts for DPR)
        let physical_size = size_canvas_backing_store(&canvas);

        // Stash window/canvas on self
        self.canvas = Some(canvas.clone());
        self.window = Some(Arc::clone(&window));

        // ---- WASM async init (no blocking) ----
        // Clone what we need into the async task.
        let state_cell = Rc::clone(&self.state); // Rc<RefCell<Option<State<'static>>>>
        let window_arc = Arc::clone(self.window.as_ref().unwrap());
        let canvas_el = canvas.clone();

        wasm_bindgen_futures::spawn_local(async move {
            // Build the State asynchronously
            let mut state_now = State::new(&window_arc, &canvas_el).await;

            // Apply the initial size (based on DPR-calculated backing store)
            state_now.resize(physical_size);

            // Keep your existing lifetime workaround
            #[allow(unsafe_code)]
            let state_static: State<'static> = unsafe { std::mem::transmute(state_now) };

            // Publish it so the event loop can start updating/rendering
            state_cell.replace(Some(state_static));

            // Kick a redraw so about_to_wait() runs a frame soon
            window_arc.request_redraw();
        });
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let mut state_opt = self.state.borrow_mut();
        let state = match state_opt.as_mut() {
            Some(s) => s,
            None => {
                // not initialized yet
                return;
            }
        };

        let Some(window) = &self.window else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            // Recompute canvas DPR size on resize/scale changes
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(canvas) = &self.canvas {
                    let new_physical = size_canvas_backing_store(canvas);
                    state.resize(new_physical);
                }
            }

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
                state.input(InputEvent::MouseMoved(position.x, position.y));
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let scroll = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y as f64,
                    winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y,
                };
                state.input(InputEvent::MouseScrolled(scroll));
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        let mut state_opt = self.state.borrow_mut();
        let state = match state_opt.as_mut() {
            Some(s) => s,
            None => {
                // not initialized yet
                return;
            }
        };
        let Some(window) = &self.window else {
            return;
        };
        state.update();
        if let Err(e) = state.render() {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::error_1(&format!("{e:?}").into());
            #[cfg(not(target_arch = "wasm32"))]
            eprintln!("{e:?}");
        }
        window.request_redraw();
    }
}

#[wasm_bindgen(start)]
pub async fn start() -> Result<(), JsValue> {
    setup_console_log();
    println!("Starting A Better World WASM...");

    let event_loop = EventLoop::new().unwrap();
    let mut app = WebApp::default();
    event_loop.run_app(&mut app).unwrap();

    Ok(())
}
