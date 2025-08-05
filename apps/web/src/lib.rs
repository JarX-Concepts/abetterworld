use abetterworld::{ABetterWorld, InputEvent, MouseButton};
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use winit::platform::web::WindowExtWebSys;
use winit::{
    event::{ElementState, Event, WindowEvent},
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::WindowBuilder,
};
use {wasm_bindgen::JsCast, web_sys};

fn setup_console_log() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Redirect `println!` and `eprintln!`
        console_log::init_with_level(log::Level::Trace).expect("error initializing log");
        // Also show panic messages in console
        console_error_panic_hook::set_once();

        log::info!("Console logging initialized");
    });
}

struct State<'window> {
    surface: wgpu::Surface<'window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    world: ABetterWorld,
}

impl<'window> State<'window> {
    async fn new(window: &'window winit::window::Window) -> Self {
        // Get the initial size from the window - ensure it's non-zero
        let mut size = window.inner_size();

        web_sys::console::log_1(
            &format!(
                "Initial window size in State::new: {}x{}",
                size.width, size.height
            )
            .into(),
        );

        // Ensure size is not zero (important for WebGPU)
        if size.width == 0 || size.height == 0 {
            size = winit::dpi::PhysicalSize::new(800, 600);
            web_sys::console::log_1(&"Using default size (800x600) in State::new".into());
        }

        let desc = wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: Default::default(),
        };
        let instance = wgpu::Instance::new(&desc);

        // Create a surface from the canvas.
        let surface = unsafe {
            instance
                .create_surface(window)
                .expect("Failed to create surface")
        };

        // Request an adapter.
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        // Request the device and queue.
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

        // Get the list of supported formats.
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        // Configure the surface.
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![surface_format],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        // Initialize the sphere renderer from render_lib.
        let world = ABetterWorld::new(&device, &config);

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
                            r: 0.1,
                            g: 0.2,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.world.get_depth_view(),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0), // far plane
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),

                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.world
                .render(&mut render_pass, &self.queue, &self.device);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();

        Ok(())
    }
}

#[wasm_bindgen(start)]
pub async fn start() -> Result<(), JsValue> {
    setup_console_log();

    println!("Starting Blue Sphere WASM...");

    // Set up better panic messages for debugging.
    console_error_panic_hook::set_once();

    // Create an event loop.
    let event_loop = EventLoop::new().unwrap();

    // Build the window with a default size
    let window = WindowBuilder::new()
        .with_title("Blue Sphere WASM")
        .with_inner_size(winit::dpi::PhysicalSize::new(800, 600))
        .build(&event_loop)
        .unwrap();

    let canvas = window.canvas().expect("Failed to get canvas");
    canvas.set_id("canvas");

    // Set explicit canvas size
    canvas.set_width(800);
    canvas.set_height(600);

    let document = web_sys::window().unwrap().document().unwrap();
    document.body().unwrap().append_child(&canvas).unwrap();

    // Get a reference to the canvas after it's been appended
    let canvas = document
        .get_element_by_id("canvas")
        .unwrap()
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .unwrap();

    // Add some basic styling to the canvas
    canvas.style().set_property("display", "block").unwrap();
    canvas.style().set_property("margin", "auto").unwrap();

    // Force the dimensions to be applied and give browser a moment to update
    let size_str = format!("{}px", 800);
    canvas.style().set_property("width", &size_str).unwrap();
    canvas.style().set_property("height", &size_str).unwrap();

    // Log the actual canvas size for debugging
    web_sys::console::log_1(
        &format!("Canvas dimensions: {}x{}", canvas.width(), canvas.height()).into(),
    );

    let window = Arc::new(window);
    let window_clone = Arc::clone(&window);

    // Get the window size or use fallback instead of asserting
    let mut size = window.inner_size();
    if size.width == 0 || size.height == 0 {
        // Use fallback size if window inner_size reports zero
        size = winit::dpi::PhysicalSize::new(800, 600);
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&"Using fallback window size (800x600)".into());
    }

    web_sys::console::log_1(
        &format!(
            "Size for WebGPU initialization: {}x{}",
            size.width, size.height
        )
        .into(),
    );

    // Create State using our forced dimensions
    let mut state = State::new(&window).await;

    // Make sure to resize with our explicitly set dimensions
    state.resize(size);

    web_sys::console::log_1(
        &format!(
            "After resize: {}x{}",
            state.config.width, state.config.height
        )
        .into(),
    );

    // Run the event loop.
    event_loop
        .run(move |event, target| match event {
            Event::WindowEvent {
                ref event,
                window_id,
            } if window_id == window_clone.id() => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Resized(physical_size) => {
                    //state.resize(*physical_size);
                }
                WindowEvent::ScaleFactorChanged { .. } => {
                    //state.resize(window_clone.inner_size());
                }
                WindowEvent::KeyboardInput {
                    event:
                        winit::event::KeyEvent {
                            physical_key: PhysicalKey::Code(KeyCode::Escape),
                            state: ElementState::Pressed,
                            ..
                        },
                    ..
                } => {
                    target.exit();
                }
                WindowEvent::MouseInput {
                    state: button_state,
                    button,
                    ..
                } => {
                    let mapped_button = match button {
                        winit::event::MouseButton::Left => Some(MouseButton::Left),
                        winit::event::MouseButton::Right => Some(MouseButton::Right),
                        winit::event::MouseButton::Middle => Some(MouseButton::Middle),
                        _ => None,
                    };

                    if let Some(btn) = mapped_button {
                        match button_state {
                            ElementState::Pressed => {
                                state.input(InputEvent::MouseButtonPressed(btn))
                            }
                            ElementState::Released => {
                                state.input(InputEvent::MouseButtonReleased(btn))
                            }
                        }
                    }
                }

                WindowEvent::CursorMoved { position, .. } => {
                    let (x, y) = (position.x as f32, position.y as f32);
                    state.input(InputEvent::MouseMoved(x, y));
                }

                WindowEvent::MouseWheel { delta, .. } => {
                    let scroll_delta = match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => *y as f32,
                        winit::event::MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                    };
                    state.input(InputEvent::MouseScrolled(scroll_delta));
                }
                _ => {}
            },
            Event::AboutToWait => {
                state.update();
                if let Err(e) = state.render() {
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::error_1(&format!("{:?}", e).into());
                    #[cfg(not(target_arch = "wasm32"))]
                    eprintln!("{:?}", e);
                }
                window_clone.request_redraw();
            }
            _ => {}
        })
        .unwrap_or_else(|e| eprintln!("Error in event loop: {:?}", e));

    Ok(())
}
