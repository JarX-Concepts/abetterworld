use std::{error::Error, process::exit};

use cgmath::{Deg, Point3, Vector3};
mod camera;
pub mod ffi;
mod tiles;
use camera::{Camera, Uniforms};
mod cache;
use cache::TILESET_CACHE;
mod content;
use content::{DebugVertex, Vertex};
use serde::de;
use tiles::TileContent;
use wgpu::util::DeviceExt;
mod importer;
mod input;

pub struct UniformDataBlob {
    pub data: Vec<u8>,
    pub size: usize,
    pub aligned_uniform_size: usize,
    pub max_objects: usize,
    pub uniform_buffer: wgpu::Buffer,
    pub uniform_bind_group: wgpu::BindGroup,
}

pub struct SphereRenderer {
    pipeline: wgpu::RenderPipeline,
    debug_pipeline: wgpu::RenderPipeline,
    transforms: UniformDataBlob,
    camera_uniform_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    camera: Camera,
    debug_camera: Camera,
    aligned_uniform_size: usize,
    depth_view: wgpu::TextureView,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    content: TileContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    W,
    A,
    S,
    D,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ZoomIn,
    ZoomOut,
    Shift,
    Ctrl,
    Alt,
    Escape,
    // Add more as needed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug)]
pub enum InputEvent {
    KeyPressed(Key),
    KeyReleased(Key),
    MouseMoved { delta: (f32, f32) },
    MouseScrolled { delta: f32 },
    MouseButtonPressed(MouseButton),
    MouseButtonReleased(MouseButton),
    TouchStart { id: u64, position: (f32, f32) },
    TouchMove { id: u64, position: (f32, f32) },
    TouchEnd { id: u64 },
}

impl SphereRenderer {
    /// Creates a new SphereRenderer.
    pub async fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
    ) -> Self {
        //download_test();

        // Load WGSL shader from file.
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sphere Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let radius = 6_378_137.0;
        let distance: f64 = radius * 2.0;

        let x1 = -2609.581 * 1000.0;
        let y1 = -4575.442 * 1000.0;
        let z1 = 3584.967 * 1000.0;

        let x = -2609.503 * 1000.0;
        let y = -4575.306 * 1000.0;
        let z = 3584.86 * 1000.0;

        let eye = Point3::new(x1, z1, -y1);
        let target = Point3::new(0.0, 0.0, 0.0);
        let up = Vector3::unit_y();
        let camera = Camera::new(Deg(45.0), 1.0, eye, target, up);

        let debug_eye = Point3::new(x, z, -y);
        let debug_camera = Camera::new(Deg(45.0), 1.0, debug_eye, target, up);
        println!("camera: {:?}", camera.uniform());

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

        let max_objects = 1024;
        let alignment = device.limits().min_uniform_buffer_offset_alignment as usize;
        let uniform_size = std::mem::size_of::<Uniforms>();

        fn align_to(value: usize, alignment: usize) -> usize {
            (value + alignment - 1) / alignment * alignment
        }
        let aligned_uniform_size = align_to(uniform_size, alignment);

        let buffer_size = aligned_uniform_size * max_objects;

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Dynamic Uniform Buffer"),
            size: buffer_size as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut buffer_data = vec![0u8; buffer_size];

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
                        min_binding_size: Some(
                            std::num::NonZeroU64::new(camera_uniform_size).unwrap(),
                        ),
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

        // Create pipeline layout.
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[
                &camera_bind_group_layout,
                &uniform_bind_group_layout,
                &texture_bind_group_layout,
            ],
            push_constant_ranges: &[],
        });

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
                    blend: Some(wgpu::BlendState::REPLACE),
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
                polygon_mode: wgpu::PolygonMode::Line,
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

        let debug_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sphere Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("debug_shader.wgsl").into()),
        });

        let debug_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Debug Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &debug_shader,
                entry_point: Some("vs_main"),
                buffers: &[DebugVertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &debug_shader,
                entry_point: Some("main_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            cache: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus, // match your texture
                depth_write_enabled: false,               // write to the depth buffer
                depth_compare: wgpu::CompareFunction::Less, // typical for 3D
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

        Self {
            pipeline,
            debug_pipeline,
            transforms: UniformDataBlob {
                data: buffer_data,
                size: buffer_size,
                aligned_uniform_size: aligned_uniform_size,
                max_objects,
                uniform_buffer: uniform_buffer,
                uniform_bind_group: uniform_bind_group,
            },
            camera_uniform_buffer,
            camera_bind_group,
            camera,
            debug_camera,
            aligned_uniform_size,
            depth_view,
            texture_bind_group_layout,
            content: TileContent::new().unwrap(),
        }
    }

    pub fn get_depth_view(&self) -> &wgpu::TextureView {
        &self.depth_view
    }

    pub fn render<'a>(
        &'a mut self,
        render_pass: &mut wgpu::RenderPass<'a>,
        queue: &wgpu::Queue,
        device: &wgpu::Device,
    ) {
        let camera_vp = self.camera.uniform();
        queue.write_buffer(
            &self.camera_uniform_buffer,
            0,
            bytemuck::bytes_of(&camera_vp),
        );
        render_pass.set_bind_group(0, &self.camera_bind_group, &[]);

        if (self.content.latest_render.is_empty()) {
            println!("No content to render");
            return;
        }

        render_pass.set_pipeline(&self.pipeline);

        let mut counter = 0;
        queue.write_buffer(&self.transforms.uniform_buffer, 0, &self.transforms.data);

        for tile in &self.content.latest_render {
            for (i, node) in tile.nodes.iter().enumerate() {
                render_pass.set_bind_group(
                    1,
                    &self.transforms.uniform_bind_group,
                    &[counter * self.aligned_uniform_size as u32],
                );
                counter += 1;

                for mesh_index in node.mesh_indices.iter() {
                    if (*mesh_index as usize) >= tile.meshes.len() {
                        //println!("Mesh index out of bounds: {}", mesh_index);
                        continue;
                    }
                    let mesh = &tile.meshes[*mesh_index];

                    // Set the vertex and index buffer for this mesh
                    render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    render_pass
                        .set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);

                    // Set the correct material/texture bind group
                    if let Some(material_index) = mesh.material_index {
                        let material = &tile.materials[material_index];
                        if let Some(texture_index) = material.base_color_texture_index {
                            let texture_resource = &tile.textures[texture_index];
                            // You must have created the bind_group for this texture previously!
                            render_pass.set_bind_group(2, &texture_resource.bind_group, &[]);
                        }
                    }

                    // Draw call for this mesh
                    render_pass.draw_indexed(0..mesh.num_indices, 0, 0..1);
                }
            }
        }

        self.draw_debug_camera(render_pass, queue, device);
    }

    pub fn draw_debug_camera(
        &mut self,
        render_pass: &mut wgpu::RenderPass<'_>,
        queue: &wgpu::Queue,
        device: &wgpu::Device,
    ) {
        let corners = self.debug_camera.frustum_corners();
        let frustum_vertices: Vec<[f32; 3]> = corners
            .iter()
            .map(|p| [p.x as f32, p.y as f32, p.z as f32])
            .collect();

        const FRUSTUM_EDGES: [(u16, u16); 12] = [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ];
        let frustum_indices: Vec<u16> = FRUSTUM_EDGES.iter().flat_map(|&(a, b)| [a, b]).collect();

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Frustum Vertices"),
            contents: bytemuck::cast_slice(&frustum_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Frustum Indices"),
            contents: bytemuck::cast_slice(&frustum_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        render_pass.set_pipeline(&self.debug_pipeline);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        render_pass.draw_indexed(0..frustum_indices.len() as u32, 0, 0..1);
    }

    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<(), Box<dyn Error>> {
        //self.camera.yaw(Deg(2.0));
        //self.camera.zoom(5000.0);
        self.camera.update(None);

        self.debug_camera.update(Some(20000.0));

        self.content.update_in_range(&self.debug_camera)?;
        let state = self.content.update_loaded();
        if state.is_err() {
            eprintln!("Error updating content: {:?}", state.err());
            exit(1);
        }
        self.content
            .update_render(device, queue, &self.texture_bind_group_layout)?;

        let mut counter = 0;
        for tile in &self.content.latest_render {
            for (i, node) in tile.nodes.iter().enumerate() {
                let matrix_bytes = bytemuck::bytes_of(&node.matrix);

                let start = counter * self.aligned_uniform_size;
                let end = start + matrix_bytes.len();

                self.transforms.data[start..end].copy_from_slice(matrix_bytes);

                counter += 1;
            }
        }

        Ok(())
    }

    pub fn input(&mut self, event: InputEvent) {
        input::process_input(&mut self.camera, event);
    }
}
