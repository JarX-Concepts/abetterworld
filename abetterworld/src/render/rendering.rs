use wgpu::util::DeviceExt;

use crate::{
    content::{DebugVertex, Vertex},
    helpers::{uniform_size, Uniforms},
};

pub struct UniformDataBlob {
    pub data: Vec<u8>,
    pub size: usize,
    pub aligned_uniform_size: usize,
    pub max_objects: usize,
    pub uniform_buffer: wgpu::Buffer,
    pub uniform_bind_group: wgpu::BindGroup,
}

pub struct RenderPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub transforms: UniformDataBlob,
    pub texture_bind_group_layout: Option<wgpu::BindGroupLayout>,
}

pub fn build_pipeline(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> RenderPipeline {
    let texture_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Texture Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

    let max_objects = 600;
    let alignment = device.limits().min_uniform_buffer_offset_alignment as usize;
    let aligned_uniform_size = uniform_size(alignment);
    let buffer_size = aligned_uniform_size * max_objects;

    let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Dynamic Uniform Buffer"),
        size: buffer_size as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Create bind group layout and bind group for the uniform.
    let uniform_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Uniform Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: Some(
                        std::num::NonZeroU64::new(aligned_uniform_size as u64).unwrap(),
                    ),
                },
                count: None,
            }],
        });

    let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Uniform Bind Group"),
        layout: &uniform_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &uniform_buffer,
                offset: 0,
                size: Some(std::num::NonZeroU64::new(aligned_uniform_size as u64).unwrap()),
            }),
        }],
    });

    /*
    // Size of one matrix
    let camera_uniform_size = std::mem::size_of::<Uniforms>() as wgpu::BufferAddress;

    // Create uniform buffer for camera VP matrix
    let camera_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Camera Uniform Buffer"),
        size: camera_uniform_size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let camera_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Camera Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(std::num::NonZeroU64::new(camera_uniform_size).unwrap()),
                },
                count: None,
            }],
        });

    let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Camera Bind Group"),
        layout: &camera_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: camera_uniform_buffer.as_entire_binding(),
        }],
    });
    */

    // Create pipeline layout.
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[&uniform_bind_group_layout, &texture_bind_group_layout],
        push_constant_ranges: &[],
    });

    // Load WGSL shader from file.
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Sphere Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../assets/shader.wgsl").into()),
    });

    // Create the render pipeline.
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[Vertex::desc()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: config.format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        cache: None,
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24Plus, // match your texture
            depth_write_enabled: true,                // write to the depth buffer
            depth_compare: wgpu::CompareFunction::LessEqual, // typical for 3D
            stencil: wgpu::StencilState::default(),   // usually default
            bias: wgpu::DepthBiasState::default(),    // optional slope‐bias
        }),
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
    });

    let buffer_data = vec![0u8; buffer_size];

    RenderPipeline {
        pipeline,
        texture_bind_group_layout: Some(texture_bind_group_layout),
        transforms: UniformDataBlob {
            data: buffer_data,
            size: buffer_size,
            aligned_uniform_size: aligned_uniform_size,
            max_objects,
            uniform_buffer: uniform_buffer,
            uniform_bind_group: uniform_bind_group,
        },
    }
}

pub struct DepthBuffer {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

pub fn build_depth_buffer(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> DepthBuffer {
    let depth_size = wgpu::Extent3d {
        width: config.width,
        height: config.height,
        depth_or_array_layers: 1,
    };

    let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Depth Texture"),
        size: depth_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24Plus, // or Depth32Float
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[wgpu::TextureFormat::Depth24Plus],
    });

    let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

    DepthBuffer {
        texture: depth_texture,
        view: depth_view,
    }
}

pub fn build_debug_pipeline(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> RenderPipeline {
    let debug_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Sphere Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../assets/debug_shader.wgsl").into()),
    });

    // Size of one matrix
    let camera_uniform_size = std::mem::size_of::<Uniforms>() as wgpu::BufferAddress;

    // Create uniform buffer for camera VP matrix
    let camera_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Camera Uniform Buffer"),
        size: camera_uniform_size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let camera_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Camera Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(std::num::NonZeroU64::new(camera_uniform_size).unwrap()),
                },
                count: None,
            }],
        });

    let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Camera Bind Group"),
        layout: &camera_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: camera_uniform_buffer.as_entire_binding(),
        }],
    });

    let debug_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[&camera_bind_group_layout],
        push_constant_ranges: &[],
    });

    let debug_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Debug Pipeline"),
        layout: Some(&debug_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &debug_shader,
            entry_point: Some("vs_main"),
            buffers: &[DebugVertex::desc()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &debug_shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: config.format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),

        cache: None,
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24Plus,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
    });

    let alignment = device.limits().min_uniform_buffer_offset_alignment as usize;
    let aligned_uniform_size = uniform_size(alignment);

    RenderPipeline {
        pipeline: debug_pipeline,
        texture_bind_group_layout: None,
        transforms: UniformDataBlob {
            data: vec![0u8; aligned_uniform_size],
            size: aligned_uniform_size * 1,
            aligned_uniform_size: aligned_uniform_size,
            max_objects: 1,
            uniform_buffer: camera_uniform_buffer,
            uniform_bind_group: camera_bind_group,
        },
    }
}

pub struct FrustumRender {
    pub vertex_buffer: wgpu::Buffer,
    pub volume_buffer: wgpu::Buffer,
    pub frustum_buffer: wgpu::Buffer,
}

pub const MAX_VOLUMES: u64 = 512;
pub const SIZE_OF_VOLUME: u64 = 8 * std::mem::size_of::<DebugVertex>() as u64;

pub fn build_frustum_render(device: &wgpu::Device) -> FrustumRender {
    const FRUSTUM_TRI_INDICES: [u16; 36] = [
        // Near
        0, 1, 2, 2, 3, 0, // Far
        4, 5, 6, 6, 7, 4, // Left
        0, 3, 7, 7, 4, 0, // Right
        1, 5, 6, 6, 2, 1, // Top
        0, 4, 5, 5, 1, 0, // Bottom
        3, 2, 6, 6, 7, 3,
    ];

    const VOLUME_TRI_INDICES: [u16; 36] = [
        // Near face (-Z)
        0, 1, 5, 5, 4, 0, // Far face (+Z)
        2, 3, 7, 7, 6, 2, // Left face (-X)
        0, 2, 6, 6, 4, 0, // Right face (+X)
        1, 3, 7, 7, 5, 1, // Bottom face (-Y)
        0, 1, 3, 3, 2, 0, // Top face (+Y)
        4, 5, 7, 7, 6, 4,
    ];

    let volume_indices: Vec<u16> = VOLUME_TRI_INDICES.iter().flat_map(|&i| [i]).collect();
    let frustum_indices: Vec<u16> = FRUSTUM_TRI_INDICES.iter().flat_map(|&i| [i]).collect();

    let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Frustum Vertices"),
        size: MAX_VOLUMES * SIZE_OF_VOLUME,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let volume_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Volume Indices"),
        contents: bytemuck::cast_slice(&volume_indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    let frustum_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Frustum Indices"),
        contents: bytemuck::cast_slice(&frustum_indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    FrustumRender {
        vertex_buffer,
        volume_buffer,
        frustum_buffer,
    }
}
