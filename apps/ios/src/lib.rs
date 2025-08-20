use std::ffi::c_void;

use abetterworld::{ABetterWorld, InputEvent};
use core_graphics::geometry::CGSize;
use metal::{self, MTLRegion};
use objc::{self, msg_send, runtime::Object, sel, sel_impl};
use wgpu::{self};

use crate::logging::init_logger;
mod logging;

#[repr(C)]
pub struct ABetterWorldiOS {
    _private: [u8; 0], // Prevent C/Swift from seeing the internals
}

pub struct State {
    pub inner: Option<StateInner>,
}

struct StateInner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: (u32, u32),
    abw: ABetterWorld,
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    metal_layer: *mut Object,
}

fn get_state<'a>(ptr: *mut ABetterWorldiOS) -> &'a mut State {
    assert!(!ptr.is_null());
    unsafe { &mut *(ptr as *mut State) }
}

fn get_state_inner<'a>(ptr: *mut ABetterWorldiOS) -> &'a mut StateInner {
    assert!(!ptr.is_null());
    unsafe { &mut *(ptr as *mut State) }.inner.as_mut().unwrap()
}

#[no_mangle]
pub extern "C" fn abetterworld_ios_new() -> *mut ABetterWorldiOS {
    let _ = init_logger();

    let state: Box<State> = Box::new(State { inner: None });

    Box::into_raw(state) as *mut ABetterWorldiOS
}

#[no_mangle]
pub extern "C" fn abetterworld_ios_free(ptr: *mut ABetterWorldiOS) {
    if !ptr.is_null() {
        unsafe { drop(Box::from_raw(ptr as *mut State)) };
    }
}
#[no_mangle]
pub extern "C" fn abetterworld_ios_init(
    ptr: *mut ABetterWorldiOS,
    metal_device_raw: *mut c_void,
    metal_layer_raw: *mut c_void,
    width: f64,
    height: f64,
) {
    let state = get_state(ptr);
    let metal_device = metal_device_raw as *mut Object;
    let metal_layer = metal_layer_raw as *mut Object;

    log::info!(
        "Initializing iOS renderer with Metal device: {:?}, layer: {:?}, size: {}x{}",
        metal_device,
        metal_layer,
        width,
        height
    );

    // Get the actual drawable size from the metal layer
    let mut drawable_size = unsafe {
        let size: CGSize = msg_send![metal_layer, drawableSize];
        (size.width as u32, size.height as u32)
    };

    if drawable_size.0 == 0 {
        drawable_size.0 = width as u32;
    }
    if drawable_size.1 == 0 {
        drawable_size.1 = height as u32;
    }

    log::info!(
        "Initializing iOS renderer with drawable size: {}x{}",
        drawable_size.0,
        drawable_size.1
    );

    // Create WGPU instance with Metal backend for iOS
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::METAL,
        ..Default::default()
    });

    // Request an adapter for the Metal backend
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("Failed to find an appropriate adapter");

    // Create the device and queue
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("iOS Metal Device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("Failed to create device");

    let texture_format = wgpu::TextureFormat::Rgba8Unorm;

    // Create a render texture with drawable size
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Render Texture"),
        size: wgpu::Extent3d {
            width: drawable_size.0,
            height: drawable_size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: texture_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Create a config for our rendering
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: texture_format,
        width: drawable_size.0,
        height: drawable_size.1,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        view_formats: vec![texture_format],
        desired_maximum_frame_latency: 2,
    };

    // Initialize sphere renderer with device and config
    let abw = ABetterWorld::new(&device, &config);

    state.inner = Some(StateInner {
        device,
        queue,
        config,
        abw,
        size: drawable_size,
        texture,
        texture_view,
        metal_layer: metal_layer as *mut Object,
    });
}

#[no_mangle]
pub extern "C" fn abetterworld_ios_resize(ptr: *mut ABetterWorldiOS, width: f64, height: f64) {
    let state = get_state_inner(ptr);

    // Get the actual drawable size from the metal layer
    let drawable_size = unsafe {
        let size: CGSize = msg_send![state.metal_layer, drawableSize];
        (size.width as u32, size.height as u32)
    };

    log::info!(
        "Resizing iOS renderer to drawable size: {}x{}",
        drawable_size.0,
        drawable_size.1
    );

    if drawable_size.0 > 0 && drawable_size.1 > 0 {
        state.size = drawable_size;
        state.config.width = drawable_size.0;
        state.config.height = drawable_size.1;

        // Create a new texture with the drawable size
        state.texture = state.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Render Texture"),
            size: wgpu::Extent3d {
                width: drawable_size.0,
                height: drawable_size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: state.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        // Update the texture view
        state.texture_view = state
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
    }
}

#[no_mangle]
pub extern "C" fn abetterworld_ios_render(ptr: *mut ABetterWorldiOS) {
    let state = get_state_inner(ptr);

    state.abw.update(&state.device, &state.queue);

    // Get the next drawable first to ensure we have a valid target
    let drawable = unsafe {
        let drawable: *mut Object = msg_send![state.metal_layer, nextDrawable];
        if drawable.is_null() {
            log::error!("Failed to get next drawable");
            return;
        }
        drawable
    };

    // Get the texture from the drawable and its dimensions
    let (metal_texture, texture_width, texture_height) = unsafe {
        let texture: *mut Object = msg_send![drawable, texture];
        if texture.is_null() {
            log::error!("Failed to get texture from drawable");
            return;
        }
        let width: u64 = msg_send![texture, width];
        let height: u64 = msg_send![texture, height];
        (texture, width, height)
    };

    // Check if we need to resize our WGPU texture
    if state.texture.size().width != texture_width as u32
        || state.texture.size().height != texture_height as u32
    {
        log::info!("Texture size mismatch, recreating WGPU texture");
        state.texture = state.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Render Texture"),
            size: wgpu::Extent3d {
                width: texture_width as u32,
                height: texture_height as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: state.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        state.texture_view = state
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
    }

    // Create command encoder
    let mut encoder = state
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

    // Render the sphere to our texture
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Sphere Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &state.texture_view,
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
                view: state.abw.get_depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0), // far plane
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),

            timestamp_writes: None,
            occlusion_query_set: None,
        });

        state
            .abw
            .render(&mut render_pass, &state.queue, &state.device);
    }

    // Create a staging buffer for the copy
    let bytes_per_pixel = 4;
    let aligned_bytes_per_row = ((texture_width as u32 * bytes_per_pixel + 255) / 256) * 256;
    let buffer_size = (aligned_bytes_per_row * texture_height as u32) as wgpu::BufferAddress;

    let staging_buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Staging Buffer"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    // Copy from texture to staging buffer
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &state.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &staging_buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(aligned_bytes_per_row),
                rows_per_image: Some(texture_height as u32),
            },
        },
        wgpu::Extent3d {
            width: texture_width as u32,
            height: texture_height as u32,
            depth_or_array_layers: 1,
        },
    );

    // Submit the commands
    state.queue.submit(std::iter::once(encoder.finish()));

    // Map the staging buffer and copy to Metal texture
    let slice = staging_buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    state.device.poll(wgpu::MaintainBase::Wait);

    unsafe {
        let data = slice.get_mapped_range();
        let region = MTLRegion::new_2d(0, 0, texture_width, texture_height);

        // Copy the data to the Metal texture
        let () = msg_send![metal_texture,
            replaceRegion:region
            mipmapLevel:0
            withBytes:data.as_ptr()
            bytesPerRow:aligned_bytes_per_row
        ];

        // Present the drawable
        let () = msg_send![drawable, present];
    }

    // Unmap the staging buffer
    staging_buffer.unmap();
}

// Swift/Objective-C bridge helpers

// Helper function to get the Swift-compatible version string
#[no_mangle]
pub extern "C" fn abetterworld_ios_version() -> *const std::os::raw::c_char {
    let version = concat!("Blue Sphere iOS v", env!("CARGO_PKG_VERSION"), "\0");
    version.as_ptr() as *const std::os::raw::c_char
}

#[no_mangle]
pub extern "C" fn abetterworld_ios_gesture_pinch(
    ptr: *mut ABetterWorldiOS,
    begin: bool,
    scale: f64,
    velocity: f64,
) {
    let state = get_state_inner(ptr);
    state.abw.input(InputEvent::GesturePinch {
        begin,
        scale,
        velocity,
    });
}

#[no_mangle]
pub extern "C" fn abetterworld_ios_gesture_pan_orbit(
    ptr: *mut ABetterWorldiOS,
    begin: bool,
    dx: f64,
    dy: f64,
    vx: f64,
    vy: f64,
) {
    let state = get_state_inner(ptr);
    state.abw.input(InputEvent::GestureOrbit {
        begin,
        dx,
        dy,
        vx,
        vy,
    });
}

#[no_mangle]
pub extern "C" fn abetterworld_ios_gesture_pan_translate(
    ptr: *mut ABetterWorldiOS,
    begin: bool,
    dx: f64,
    dy: f64,
    vx: f64,
    vy: f64,
) {
    let state = get_state_inner(ptr);
    state.abw.input(InputEvent::GestureTranslate {
        begin,
        dx,
        dy,
        vx,
        vy,
    });
}

#[no_mangle]
pub extern "C" fn abetterworld_ios_gesture_rotate(
    ptr: *mut ABetterWorldiOS,
    begin: bool,
    radians: f64,
    velocity: f64,
) {
    let state = get_state_inner(ptr);
    state.abw.input(InputEvent::GestureRotate {
        begin,
        radians,
        velocity,
    });
}

#[no_mangle]
pub extern "C" fn abetterworld_ios_gesture_double_tap(ptr: *mut ABetterWorldiOS, x: f64, y: f64) {
    let state = get_state_inner(ptr);
    state.abw.input(InputEvent::GestureDoubleTap { x, y });
}

#[no_mangle]
pub extern "C" fn abetterworld_ios_touch_down(
    ptr: *mut ABetterWorldiOS,
    active: bool,
    x: f64,
    y: f64,
) {
    let state = get_state_inner(ptr);
    state
        .abw
        .input(InputEvent::GestureTouchDown { active, x, y });
}
