use wgpu::util::DeviceExt;

use crate::{
    content::{DebugVertex, Vertex},
    helpers::{uniform_size, Uniforms},
    DepthBuffer,
};

impl DepthBuffer {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat, // e.g. Depth24Plus or Depth24PlusStencil8
        sample_count: u32,           // must match your render pipeline
    ) -> Self {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size,
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            // view_formats is typically empty for depth; include `format` only if you need
            // to create views with an alternate (compatible) format.
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            texture,
            view,
            format,
            sample_count,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        } // minimized window guard
        *self = Self::new(device, width, height, self.format, self.sample_count);
    }
}
