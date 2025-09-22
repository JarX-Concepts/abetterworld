use wgpu::util::DeviceExt;

use crate::{
    content::{DebugVertex, Vertex, MAX_RENDERABLE_NODES_US, MAX_RENDERABLE_TILES},
    helpers::Uniforms,
    render::InstanceBuffer,
};

pub struct BindingData {
    pub tile_bg: wgpu::BindGroup,
    pub tile_bg_layout: wgpu::BindGroupLayout,
    pub instance_buffer: Option<InstanceBuffer>,
    pub camera_buffer: Option<wgpu::Buffer>,
}

pub struct RenderPipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub bindings: BindingData,
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

    // Create bind group layout and bind group for the uniform.
    let tile_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Tile Bind Group Layout"),
            entries: &[
                // binding(0) camera
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(
                            std::mem::size_of::<Uniforms>() as u64,
                        ),
                    },
                    count: None,
                },
                // binding(1) instances STORAGE (read-only)
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

    let camera_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("camera_ubo"),
        size: std::mem::size_of::<Uniforms>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let instance_buffer = InstanceBuffer::new(device, MAX_RENDERABLE_NODES_US);

    let tile_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Tile Bind Group"),
        layout: &tile_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: instance_buffer.buf.as_entire_binding(),
            },
        ],
    });

    // Create pipeline layout.
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Tile Render Pipeline Layout"),
        bind_group_layouts: &[&tile_bind_group_layout, &texture_bind_group_layout],
        push_constant_ranges: &[],
    });

    // Load WGSL shader from file.
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Tile Shader"),
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

    RenderPipeline {
        pipeline,
        texture_bind_group_layout: Some(texture_bind_group_layout),
        bindings: BindingData {
            tile_bg: tile_bind_group,
            tile_bg_layout: tile_bind_group_layout,
            instance_buffer: Some(instance_buffer),
            camera_buffer: Some(camera_buf),
        },
    }
}

pub fn rebuild_tile_bg(device: &wgpu::Device, pipeline: &mut RenderPipeline) {
    let new_tile_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Tile Bind Group"),
        layout: &pipeline.bindings.tile_bg_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: pipeline
                    .bindings
                    .camera_buffer
                    .as_ref()
                    .unwrap()
                    .as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: pipeline
                    .bindings
                    .instance_buffer
                    .as_ref()
                    .unwrap()
                    .buf
                    .as_entire_binding(),
            },
        ],
    });
    pipeline.bindings.tile_bg = new_tile_bg;
}

pub fn build_debug_pipeline(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> RenderPipeline {
    let debug_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Debug Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../assets/debug_shader.wgsl").into()),
    });

    // Size of one matrix
    let camera_uniform_size = std::mem::size_of::<Uniforms>() as wgpu::BufferAddress;

    // Create uniform buffer for camera VP matrix
    let camera_uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Debug Uniform Buffer"),
        size: camera_uniform_size,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let camera_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Debug Bind Group Layout"),
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
        label: Some("Debug Bind Group"),
        layout: &camera_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: camera_uniform_buffer.as_entire_binding(),
        }],
    });

    let debug_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Debug Render Pipeline Layout"),
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

    RenderPipeline {
        pipeline: debug_pipeline,
        texture_bind_group_layout: None,
        bindings: BindingData {
            tile_bg_layout: camera_bind_group_layout,
            tile_bg: camera_bind_group,
            instance_buffer: None,
            camera_buffer: Some(camera_uniform_buffer),
        },
    }
}

pub struct FrustumRender {
    pub vertex_buffer: wgpu::Buffer,
    pub volume_buffer: wgpu::Buffer,
    pub frustum_buffer: wgpu::Buffer,
}

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
        size: MAX_RENDERABLE_TILES * SIZE_OF_VOLUME,
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
