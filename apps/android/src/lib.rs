// src/lib.rs
#![allow(non_snake_case)]

use abetterworld::ABetterWorld;
use jni::objects::{JClass, JObject};
use jni::sys::{jint, jlong, jobject};
use jni::JNIEnv;
use wgpu::Error;

use ndk::native_window::NativeWindow;
use wgpu::{self, util::DeviceExt};

use std::ffi::c_void;
use std::ptr::NonNull;

use crate::logging::init_logger;
mod logging;

use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle,
};

struct AndroidWindow {
    native_window: NativeWindow, // keep alive as long as the surface lives
}

impl HasWindowHandle for AndroidWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: ndk::NativeWindow guarantees a valid non-null ANativeWindow*
        let nn = unsafe {
            NonNull::new(self.native_window.ptr().as_ptr().cast::<c_void>())
                .ok_or(HandleError::Unavailable)?
        };
        let wnd = AndroidNdkWindowHandle::new(nn);
        // SAFETY: the returned handle borrows from &self; self outlives the borrow
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AndroidNdk(wnd)) })
    }
}

impl HasDisplayHandle for AndroidWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let dh = AndroidDisplayHandle::new();
        // SAFETY: borrows from &self (lifetime is tied to &self)
        Ok(unsafe { DisplayHandle::borrow_raw(RawDisplayHandle::Android(dh)) })
    }
}

struct GfxState {
    _instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    // Keep window alive so surface handle stays valid
    _window: Box<AndroidWindow>,
    abw: ABetterWorld,
}

pub struct State {
    gfx: Option<GfxState>,
}

#[no_mangle]
pub extern "C" fn Java_com_jarxconcepts_abetterworld_Renderer_nativeCreateState(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    std::env::set_var(
        "RUST_LOG",
        "wgpu_core=trace,wgpu_hal=trace,wgpu=trace,naga=info",
    );
    let _ = init_logger().expect("Logger initialization failed");

    log::info!("Creating native state for Android A Better World");

    let state = Box::new(State { gfx: None });
    Box::into_raw(state) as jlong
}

#[no_mangle]
pub extern "C" fn Java_com_jarxconcepts_abetterworld_Renderer_nativeDestroyState(
    _env: JNIEnv,
    _class: JClass,
    state_ptr: jlong,
) {
    if state_ptr == 0 {
        return;
    }
    unsafe {
        let _ = Box::from_raw(state_ptr as *mut State);
    }
}

#[no_mangle]
pub extern "C" fn Java_com_jarxconcepts_abetterworld_Renderer_nativeInitRenderer(
    mut env: JNIEnv,
    _class: JClass,
    state_ptr: jlong,
    surface_obj: jobject, // android.view.Surface
    width: jint,
    height: jint,
) {
    let state = unsafe { &mut *(state_ptr as *mut State) };

    // In nativeInitRenderer: build the NativeWindow correctly
    let native_window = unsafe {
        // use raw JNI pointers as required by ndk
        NativeWindow::from_surface(env.get_raw(), surface_obj)
            .expect("ANativeWindow_fromSurface failed")
    };
    let window = Box::new(AndroidWindow { native_window });

    //let instance = wgpu::Instance::default();

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN, // <- force GLES on Android/emulator
        flags: wgpu::InstanceFlags::empty(), // <- don't ask for validation layers
        ..Default::default()
    });

    // Surface creation: unwrap the Result from from_window
    let target = unsafe {
        wgpu::SurfaceTargetUnsafe::from_window(&*window)
            .expect("from_window handle creation failed")
    };
    let surface = unsafe {
        instance
            .create_surface_unsafe(target)
            .expect("create_surface_unsafe failed")
    };

    // 4) Pick adapter compatible with this surface
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .expect("No suitable adapter");

    // 5) Device/Queue
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Android Device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off, // consider enabling later
    }))
    .unwrap();

    device.on_uncaptured_error(Box::new(move |e: Error| match e {
        Error::Validation { description, .. } => {
            log::error!("WGPU validation error: {description}");
        }
        Error::OutOfMemory { source } => {
            log::error!("WGPU OOM: {source:?}");
        }
        other => {
            log::error!("WGPU uncaptured error: {other:?}");
        }
    }));

    // 6) Surface config (choose supported format/modes)
    let caps = surface.get_capabilities(&adapter);

    log::info!(
        "caps: formats={:?} present_modes={:?} alpha_modes={:?}",
        caps.formats,
        caps.present_modes,
        caps.alpha_modes
    );

    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| {
            matches!(
                f,
                wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
            )
        })
        .unwrap_or(caps.formats[0]);

    let present_mode = wgpu::PresentMode::Fifo;
    let alpha_mode = wgpu::CompositeAlphaMode::Inherit;

    // After you have `adapter`
    let limits = adapter.limits(); // SupportedLimits
    let max_dim = 2048;

    let mut w = (width as u32).max(1);
    let mut h = (height as u32).max(1);

    if w > max_dim || h > max_dim {
        let scale = f32::min(max_dim as f32 / w as f32, max_dim as f32 / h as f32);
        w = ((w as f32) * scale).floor() as u32;
        h = ((h as f32) * scale).floor() as u32;
        log::warn!(
            "Clamping surface from {}x{} to {}x{} (max_dim={})",
            width,
            height,
            w,
            h,
            max_dim
        );
    }

    // Diagnose configure-time validation errors explicitly
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: w,
        height: h,
        present_mode,
        alpha_mode,
        view_formats: vec![], // <-- IMPORTANT on GL
        desired_maximum_frame_latency: 1,
    };
    surface.configure(&device, &config);
    if let Some(err) = pollster::block_on(device.pop_error_scope()) {
        log::error!("surface.configure validation error: {err}");
    }

    let abw = ABetterWorld::new(&device, &config);

    state.gfx = Some(GfxState {
        _instance: instance,
        surface,
        adapter,
        device,
        queue,
        config,
        _window: window, // keep alive
        abw,
    });
}

#[no_mangle]
pub extern "C" fn Java_com_jarxconcepts_abetterworld_Renderer_nativeResize(
    _env: JNIEnv,
    _class: JClass,
    state_ptr: jlong,
    width: jint,
    height: jint,
) {
    let state = unsafe { &mut *(state_ptr as *mut State) };
    let Some(g) = &mut state.gfx else {
        return;
    };

    if width <= 0 || height <= 0 {
        return;
    }
    g.config.width = width as u32;
    g.config.height = height as u32;
    g.surface.configure(&g.device, &g.config);
}

#[no_mangle]
pub extern "C" fn Java_com_jarxconcepts_abetterworld_Renderer_nativeRender(
    _env: JNIEnv,
    _class: JClass,
    state_ptr: jlong,
) {
    let state = unsafe { &mut *(state_ptr as *mut State) };

    let Some(g) = &mut state.gfx else {
        return;
    };

    g.abw.update(&g.device, &g.queue);

    // Acquire a frame
    let frame = match g.surface.get_current_texture() {
        Ok(f) => f,
        Err(e) => {
            // Reconfigure on Outdated/Lost
            log::warn!("get_current_texture failed: {e:?}; reconfiguring");
            g.surface.configure(&g.device, &g.config);
            match g.surface.get_current_texture() {
                Ok(f) => f,
                Err(_) => return,
            }
        }
    };
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    // Simple clear pass
    let mut encoder = g
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
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
                view: g.abw.get_depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0), // far plane
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),

            occlusion_query_set: None,
            timestamp_writes: None,
        });

        g.abw.render(&mut rp, &g.queue, &g.device);
    }

    g.queue.submit([encoder.finish()]);

    frame.present();
}
