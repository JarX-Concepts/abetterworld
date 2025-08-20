use winit::{
    event::{ElementState, Event, WindowEvent},
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::WindowBuilder,
};

use abetterworld::{ABetterWorld, InputEvent, Key, MouseButton};
use std::sync::Arc;

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
        let size = window.inner_size();

        // Create wgpu instance with the new InstanceDescriptor.
        let desc = wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            backend_options: Default::default(),
        };
        let instance = wgpu::Instance::new(&desc);

        // Unwrap the surface creation.
        let surface = instance
            .create_surface(window)
            .expect("Failed to create surface");

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
                required_features: wgpu::Features::POLYGON_MODE_LINE,
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
                label: None,
                trace: wgpu::Trace::Off,
            })
            .await
            .unwrap();

        // Choose a surface format.
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps.formats[0];

        // Note: The SurfaceConfiguration type now requires alpha_mode and view_formats.
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

        // Initialize the sphere renderer from the library.
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
        self.size = new_size;
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);

        self.world
            .resize(&self.device, new_size.width, new_size.height);
    }

    fn input(&mut self, event: InputEvent) {
        // No dynamic updates for now.
        self.world.input(event);
    }

    fn update(&mut self) {
        self.world
            .update(&self.device, &self.queue)
            .map_err(|e| {
                eprintln!("Update error: {:?}", e);
            })
            .ok();
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

fn map_keycode(physical_key: &PhysicalKey) -> Option<Key> {
    match physical_key {
        PhysicalKey::Code(KeyCode::KeyW) => Some(Key::W),
        PhysicalKey::Code(KeyCode::KeyA) => Some(Key::A),
        PhysicalKey::Code(KeyCode::KeyS) => Some(Key::S),
        PhysicalKey::Code(KeyCode::KeyD) => Some(Key::D),

        PhysicalKey::Code(KeyCode::Equal) => Some(Key::ZoomIn),
        PhysicalKey::Code(KeyCode::PageDown) => Some(Key::ZoomIn),

        PhysicalKey::Code(KeyCode::PageUp) => Some(Key::ZoomOut),
        PhysicalKey::Code(KeyCode::Minus) => Some(Key::ZoomOut),

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

fn main() {
    dotenv::dotenv().ok();
    env_logger::init();
    log::info!("Starting A Better World application...");

    let event_loop = EventLoop::new().unwrap();
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("A Better World")
            .build(&event_loop)
            .unwrap(),
    );
    let window_clone = Arc::clone(&window);

    let mut state = pollster::block_on(State::new(&window));

    event_loop
        .run(move |event, target| match event {
            Event::WindowEvent {
                ref event,
                window_id,
            } if window_id == window_clone.id() => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Resized(physical_size) => {
                    state.resize(*physical_size);
                }
                WindowEvent::ScaleFactorChanged { .. } => {
                    state.resize(window_clone.inner_size());
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
                WindowEvent::KeyboardInput {
                    event:
                        winit::event::KeyEvent {
                            physical_key,
                            state: key_state,
                            ..
                        },
                    ..
                } => {
                    if let Some(key) = map_keycode(physical_key) {
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
                match state.render() {
                    Ok(_) => {}
                    Err(wgpu::SurfaceError::Lost) => state.resize(state.size),
                    Err(wgpu::SurfaceError::OutOfMemory) => target.exit(),
                    Err(e) => eprintln!("{:?}", e),
                }
                window_clone.request_redraw();
            }
            _ => {}
        })
        .unwrap_or_else(|e| eprintln!("Error in event loop: {:?}", e));
}
