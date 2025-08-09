use std::ffi::CString;
use std::os::raw::c_char;

use abetterworld::ABetterWorld;
use jni::objects::{JClass, JObject, JString};
use jni::sys::{jlong, jstring};
use jni::JNIEnv;
use wgpu::{Device, Queue};

#[repr(C)]
pub struct ABetterWorldAndroid {
    _private: [u8; 0], // Hide internals from JNI
}

pub struct State {
    pub renderer: Option<ABetterWorld>,
}

#[no_mangle]
pub extern "C" fn Java_com_jarxconcepts_abetterworld_Renderer_nativeCreateState(
    env: JNIEnv,
    _class: JClass,
) -> jlong {
    let state = Box::new(State {
        renderer: None, // Will be initialized later
    });
    Box::into_raw(state) as jlong
}

#[no_mangle]
pub extern "C" fn Java_com_jarxconcepts_abetterworld_Renderer_nativeDestroyState(
    env: JNIEnv,
    _class: JClass,
    state_ptr: jlong,
) {
    if state_ptr != 0 {
        unsafe {
            drop(Box::from_raw(state_ptr as *mut State));
        }
    }
}

#[no_mangle]
pub extern "C" fn Java_com_jarxconcepts_abetterworld_Renderer_nativeInitRenderer(
    env: JNIEnv,
    _class: JClass,
    state_ptr: jlong,
    width: i32,
    height: i32,
) {
    let state = unsafe { &mut *(state_ptr as *mut State) };

    // Mock example of device + queue creation
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();

    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Rgba8Unorm,
        width: width as u32,
        height: height as u32,
        present_mode: wgpu::PresentMode::Fifo,
        ..Default::default()
    };

    let renderer = ABetterWorld::new(&device, &config);
    state.renderer = Some(renderer);
}

#[no_mangle]
pub extern "C" fn Java_com_jarxconcepts_abetterworld_Renderer_nativeVersion(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let version_str = format!("Blue Sphere Android v{}", env!("CARGO_PKG_VERSION"));
    let output = env.new_string(version_str).unwrap();
    output.into_raw()
}
