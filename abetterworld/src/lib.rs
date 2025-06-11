use std::{
    error::Error,
    sync::{Arc, RwLock},
};

mod camera;
pub mod decode;
mod tiles;
use camera::Camera;
mod cache;
use cache::init_tileset_cache;
mod content;
use decode::init;
use pager::start_background_tasks;
use wgpu::util::DeviceExt;

use crate::{
    camera::init_camera,
    rendering::{build_debug_pipeline, build_depth_buffer, build_pipeline, RenderPipeline},
};
mod coord_utils;
mod importer;
mod input;
mod matrix;
mod pager;
mod rendering;
mod tests;

pub struct UniformDataBlob {
    pub data: Vec<u8>,
    pub size: usize,
    pub aligned_uniform_size: usize,
    pub max_objects: usize,
    pub uniform_buffer: wgpu::Buffer,
    pub uniform_bind_group: wgpu::BindGroup,
}

pub struct SphereRenderer {
    pipeline: RenderPipeline,
    debug_pipeline: wgpu::RenderPipeline,
    camera: Arc<RwLock<Camera>>,
    debug_camera: Arc<RwLock<Camera>>,
    depth_view: wgpu::TextureView,
    content: Arc<pager::TileContent>,
    input_state: input::InputState,
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
    MouseMoved(f32, f32),
    MouseScrolled(f32),
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
        init_tileset_cache().await;

        let (camera, debug_camera) = init_camera();

        let pipeline = build_pipeline(device, config);
        let debug_pipeline =
            build_debug_pipeline(device, config, &pipeline.camera_bind_group_layout);
        let depth = build_depth_buffer(device, config);

        let tile_content = Arc::new(pager::TileContent::new().unwrap());
        let camera_source = Arc::new(RwLock::new(camera));
        let debug_camera_source = Arc::new(RwLock::new(debug_camera));

        init();
        start_background_tasks(tile_content.clone(), debug_camera_source.clone()).await;

        Self {
            pipeline,
            debug_pipeline,
            camera: camera_source,
            debug_camera: debug_camera_source,
            depth_view: depth.view,
            content: tile_content,
            input_state: input::InputState::new(),
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
        let camera_vp = self.camera.read().unwrap().uniform();
        queue.write_buffer(
            &self.pipeline.camera_uniform_buffer,
            0,
            bytemuck::bytes_of(&camera_vp),
        );
        render_pass.set_bind_group(0, &self.pipeline.camera_bind_group, &[]);

        {
            let latest_render = self.content.latest_render.read().unwrap();
            if !latest_render.is_empty() {
                render_pass.set_pipeline(&self.pipeline.pipeline);

                let mut counter = 0;
                queue.write_buffer(
                    &self.pipeline.transforms.uniform_buffer,
                    0,
                    &self.pipeline.transforms.data,
                );

                for tile in latest_render.iter() {
                    for (i, node) in tile.nodes.iter().enumerate() {
                        render_pass.set_bind_group(
                            1,
                            &self.pipeline.transforms.uniform_bind_group,
                            &[counter * self.pipeline.transforms.aligned_uniform_size as u32],
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
                            render_pass.set_index_buffer(
                                mesh.index_buffer.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );

                            // Set the correct material/texture bind group
                            if let Some(material_index) = mesh.material_index {
                                let material = &tile.materials[material_index];
                                if let Some(texture_index) = material.base_color_texture_index {
                                    let texture_resource = &tile.textures[texture_index];
                                    // You must have created the bind_group for this texture previously!
                                    render_pass.set_bind_group(
                                        2,
                                        &texture_resource.bind_group,
                                        &[],
                                    );
                                }
                            }

                            // Draw call for this mesh
                            render_pass.draw_indexed(0..mesh.num_indices, 0, 0..1);
                            //render_pass.draw(0..3, 0..1);
                        }
                    }
                }
            }
        }

        //self.draw_debug_camera(render_pass, queue, device);
    }

    pub fn draw_debug_camera(
        &mut self,
        render_pass: &mut wgpu::RenderPass<'_>,
        queue: &wgpu::Queue,
        device: &wgpu::Device,
    ) {
        let corners = self.debug_camera.read().unwrap().frustum_corners();
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
        //self.debug_camera.yaw(Deg(2.0));
        //self.camera.zoom(5000.0);
        self.camera.write().unwrap().update(None);
        self.debug_camera.write().unwrap().update(Some(20000.0));

        self.content
            .update_render(device, queue, &self.pipeline.texture_bind_group_layout)?;

        let mut counter = 0;

        let latest_render = self.content.latest_render.read().unwrap();

        for tile in latest_render.iter() {
            for (i, node) in tile.nodes.iter().enumerate() {
                let matrix_bytes = bytemuck::bytes_of(&node.matrix);

                let start = counter * self.pipeline.transforms.aligned_uniform_size;
                let end = start + matrix_bytes.len();

                self.pipeline.transforms.data[start..end].copy_from_slice(matrix_bytes);

                counter += 1;
            }
        }

        Ok(())
    }

    pub fn input(&mut self, event: InputEvent) {
        self.input_state
            .process_input(&mut self.camera.write().unwrap(), event);
    }
}
