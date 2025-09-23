#[derive(Clone, Copy, Debug)]
pub enum DepthMode {
    Normal,   // Forward-Z (compare: Less, clear: 1.0)
    ReverseZ, // Reverse-Z (compare: Greater, clear: 0.0)
}

pub struct DepthBuffer {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub sample_count: u32,
    pub mode: DepthMode,
}

/// Prefer a float depth format for best reverse-Z precision; fall back to Depth24Plus.
pub fn recommended_format() -> wgpu::TextureFormat {
    // Depth32Float is widely available; Depth24Plus is guaranteed.
    wgpu::TextureFormat::Depth32Float
}

impl DepthBuffer {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat, // e.g., Depth32Float (preferred) or Depth24Plus
        sample_count: u32,
        mode: DepthMode,
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
            view_formats: &[], // depth generally doesn't need view format remaps
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            texture,
            view,
            format,
            sample_count,
            mode,
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return; // minimized window guard
        }
        *self = Self::new(
            device,
            width,
            height,
            self.format,
            self.sample_count,
            self.mode,
        );
    }

    /// Clear value appropriate for the mode (use in your render pass).
    #[inline]
    pub fn clear_value(&self) -> f32 {
        match self.mode {
            DepthMode::Normal => 1.0,   // far in forward-Z
            DepthMode::ReverseZ => 0.0, // far in reverse-Z
        }
    }

    /// Compare function appropriate for the mode (use in your pipeline).
    #[inline]
    pub fn compare_fn(&self) -> wgpu::CompareFunction {
        match self.mode {
            DepthMode::Normal => wgpu::CompareFunction::Less,
            DepthMode::ReverseZ => wgpu::CompareFunction::GreaterEqual,
        }
    }

    /// Convenience builder for a depth attachment with a clear.
    pub fn attachment_clear(&self) -> wgpu::RenderPassDepthStencilAttachment {
        wgpu::RenderPassDepthStencilAttachment {
            view: &self.view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(self.clear_value()),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }
    }

    /// Convenience builder for a depth attachment that loads existing depth.
    pub fn attachment_load(&self) -> wgpu::RenderPassDepthStencilAttachment {
        wgpu::RenderPassDepthStencilAttachment {
            view: &self.view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }
    }

    /// Depth-stencil state helper to keep pipelines consistent with the mode.
    pub fn depth_stencil_state(&self) -> Option<wgpu::DepthStencilState> {
        Some(wgpu::DepthStencilState {
            format: self.format,
            depth_write_enabled: true,
            depth_compare: self.compare_fn(),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(), // note: sign expectations flip with reverse-Z
        })
    }
}
