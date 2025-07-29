mod cache;
mod camera;
mod content;
mod coord_utils;
mod decode;
mod download;
mod errors;
mod helpers;
mod importer;
mod input;
mod matrix;
mod pager;
mod rendering;
mod tests;
mod tile_manager;
mod tiles;
mod tilesets;
mod volumes;

use crate::{
    camera::init_camera,
    content::{DebugVertex, Tile, TileState},
    matrix::is_bounding_volume_visible,
    pager::start_pager,
    rendering::{
        build_debug_pipeline, build_depth_buffer, build_frustum_render, build_pipeline,
        FrustumRender, RenderPipeline, MAX_VOLUMES, SIZE_OF_VOLUME,
    },
    tile_manager::TileManager,
    volumes::Ray,
};
use cache::init_tileset_cache;
use camera::Camera;
use cgmath::{Deg, EuclideanSpace, InnerSpace, Matrix4, SquareMatrix, Vector3, Zero};
use crossbeam_channel::{bounded, Receiver};
use decode::init;
use std::{
    error::Error,
    ops::Deref,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

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
    debug_pipeline: RenderPipeline,
    camera: Arc<Camera>,
    debug_camera: Arc<Camera>,
    depth_view: wgpu::TextureView,
    content: Arc<TileManager>,
    receiver: Receiver<Tile>,
    input_state: input::InputState,
    frustum_render: FrustumRender,
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
        init_tileset_cache();

        let (camera, debug_camera) = init_camera();

        let pipeline = build_pipeline(device, config);
        let debug_pipeline = build_debug_pipeline(device, config);
        let depth = build_depth_buffer(device, config);
        let frustum_render = build_frustum_render(device);

        let tile_content = Arc::new(TileManager::new());
        let camera_source = Arc::new(camera);
        let debug_camera_source = Arc::new(debug_camera);

        const MAX_NEW_TILES_PER_FRAME: usize = 1_000;
        let (loader_tx, render_rx) = bounded::<Tile>(MAX_NEW_TILES_PER_FRAME);

        let _ = init();
        let _ = start_pager(debug_camera_source.clone(), tile_content.clone(), loader_tx);

        Self {
            pipeline,
            debug_pipeline,
            camera: camera_source,
            debug_camera: debug_camera_source,
            depth_view: depth.view,
            content: tile_content,
            input_state: input::InputState::new(),
            frustum_render,
            receiver: render_rx,
        }
    }

    pub fn get_depth_view(&self) -> &wgpu::TextureView {
        &self.depth_view
    }

    pub fn render<'a>(
        &'a mut self,
        render_pass: &mut wgpu::RenderPass<'a>,
        queue: &wgpu::Queue,
        _device: &wgpu::Device,
    ) {
        {
            let latest_render = self.content.tileset.read().unwrap();
            let planes = self.debug_camera.planes();
            if !latest_render.is_empty() {
                render_pass.set_pipeline(&self.pipeline.pipeline);

                let mut counter = 0;
                queue.write_buffer(
                    &self.pipeline.transforms.uniform_buffer,
                    0,
                    &self.pipeline.transforms.data,
                );

                for tile in latest_render.iter() {
                    if let TileState::Renderable {
                        ref nodes,
                        ref meshes,
                        ref textures,
                        ref materials,
                        unload: ref _unload,
                        ref culling_volume,
                    } = tile.1.state
                    {
                        let render_it = is_bounding_volume_visible(&planes, &culling_volume);

                        if !render_it {
                            counter += nodes.len() as u32;
                            continue;
                        }

                        for (i, node) in nodes.iter().enumerate() {
                            render_pass.set_bind_group(
                                0,
                                &self.pipeline.transforms.uniform_bind_group,
                                &[counter * self.pipeline.transforms.aligned_uniform_size as u32],
                            );
                            counter += 1;

                            for mesh_index in node.mesh_indices.iter() {
                                if (*mesh_index as usize) >= meshes.len() {
                                    //println!("Mesh index out of bounds: {}", mesh_index);
                                    continue;
                                }
                                let mesh = &meshes[*mesh_index];

                                // Set the vertex and index buffer for this mesh
                                render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                                render_pass.set_index_buffer(
                                    mesh.index_buffer.slice(..),
                                    wgpu::IndexFormat::Uint32,
                                );

                                // Set the correct material/texture bind group
                                if let Some(material_index) = mesh.material_index {
                                    let material = &materials[material_index];
                                    if let Some(texture_index) = material.base_color_texture_index {
                                        let texture_resource = &textures[texture_index];
                                        // You must have created the bind_group for this texture previously!
                                        render_pass.set_bind_group(
                                            1,
                                            &texture_resource.bind_group,
                                            &[],
                                        );
                                    }
                                }

                                // Draw call for this mesh
                                render_pass.draw_indexed(0..mesh.num_indices, 0, 0..1);
                            }
                        }
                    }
                }
            }
        }

        self.draw_debug_camera(render_pass, queue);
        self.draw_all_tile_volumes(render_pass, queue);
    }

    fn draw_all_tile_volumes(&self, render_pass: &mut wgpu::RenderPass<'_>, queue: &wgpu::Queue) {
        let camera_vp = self.camera.uniform();
        queue.write_buffer(
            &self.debug_pipeline.transforms.uniform_buffer,
            0,
            bytemuck::bytes_of(&camera_vp),
        );

        render_pass.set_bind_group(0, &self.debug_pipeline.transforms.uniform_bind_group, &[]);
        render_pass.set_pipeline(&self.debug_pipeline.pipeline);
        render_pass.set_index_buffer(
            self.frustum_render.volume_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );
        render_pass.set_vertex_buffer(0, self.frustum_render.vertex_buffer.slice(..));

        // start past the debug camera
        let mut volume_counter = 1;
        let latest_render = self.content.tileset.read().unwrap();
        for _tile in latest_render.iter() {
            if volume_counter >= MAX_VOLUMES {
                //eprintln!("Hit maximum number of volumes");
            } else {
                render_pass.draw_indexed(0..36, volume_counter as i32 * 8, 0..1);
            }
            volume_counter += 1;
        }
    }

    pub fn draw_debug_camera(&self, render_pass: &mut wgpu::RenderPass<'_>, queue: &wgpu::Queue) {
        let mut camera_vp = self.camera.uniform();
        camera_vp.free_space = 0.5;

        queue.write_buffer(
            &self.debug_pipeline.transforms.uniform_buffer,
            0,
            bytemuck::bytes_of(&camera_vp),
        );

        render_pass.set_bind_group(0, &self.debug_pipeline.transforms.uniform_bind_group, &[]);
        render_pass.set_pipeline(&self.debug_pipeline.pipeline);

        let corners = self.debug_camera.frustum_corners();
        let new_frustum_vertices: Vec<DebugVertex> = corners
            .iter()
            .map(|p| DebugVertex {
                position: [p.x as f32, p.y as f32, p.z as f32],
                color: [1.0, 0.0, 0.0, 1.0],
            })
            .collect();

        queue.write_buffer(
            &self.frustum_render.vertex_buffer,
            0,
            bytemuck::cast_slice(&new_frustum_vertices),
        );

        render_pass.set_vertex_buffer(0, self.frustum_render.vertex_buffer.slice(..));
        render_pass.set_index_buffer(
            self.frustum_render.frustum_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );
        render_pass.draw_indexed(0..36, 0, 0..1);
    }

    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<(), Box<dyn Error>> {
        self.debug_camera.deref().update(None);

        const BUDGET: Duration = Duration::from_millis(20);

        if let Some(layout) = self.pipeline.texture_bind_group_layout.as_ref() {
            let deadline = Instant::now() + BUDGET;

            self.content.unload_tiles();

            // Pull tiles until either the channel is empty or we run out of time.
            while Instant::now() < deadline {
                match self.receiver.try_recv() {
                    Ok(mut tile) => {
                        if let Err(e) =
                            tiles::content_render_setup(device, queue, layout, &mut tile)
                        {
                            log::error!("Failed to set up tile for rendering: {e}");
                            continue; // skip adding this tile
                        }
                        self.content.add_tile(tile);
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => break, // nothing left
                    Err(crossbeam_channel::TryRecvError::Disconnected) => break, // sender gone
                }
            }
        }

        //self.debug_camera.yaw(Deg(0.1));
        //self.debug_camera.write().unwrap().zoom(-500.0);
        self.debug_camera.update(None);

        let mut min_distance = f64::MAX;
        {
            let latest_render = self.content.tileset.read().unwrap();
            let camera_pos = self.camera.eye_vector();
            // start past the debug camera
            let mut volume_counter = 1;

            for tile in latest_render.iter() {
                if let TileState::Renderable { culling_volume, .. } = &tile.1.state {
                    let distance = culling_volume.ray_intersect(&Ray {
                        origin: camera_pos,
                        direction: -camera_pos.normalize(),
                    });
                    if distance.is_some() && distance.unwrap() < min_distance {
                        min_distance = distance.unwrap();
                    }

                    let new_frustum_vertices: Vec<DebugVertex> = culling_volume
                        .corners
                        .iter()
                        .map(|p| DebugVertex {
                            position: [p.x as f32, p.y as f32, p.z as f32],
                            color: [1.0, 1.0, 0.25, 0.1],
                        })
                        .collect();

                    if volume_counter >= MAX_VOLUMES {
                        //eprintln!("Hit maximum number of volumes");
                    } else {
                        queue.write_buffer(
                            &self.frustum_render.vertex_buffer,
                            volume_counter * SIZE_OF_VOLUME,
                            bytemuck::cast_slice(&new_frustum_vertices),
                        );
                        volume_counter += 1;
                    }
                }
            }
        }
        let projected_cam = self.camera.update(Some(min_distance));

        {
            let latest_render = self.content.tileset.read().unwrap();

            let mut counter = 0;
            for tile in latest_render.iter() {
                if let TileState::Renderable { ref nodes, .. } = tile.1.state {
                    for (_i, node) in nodes.iter().enumerate() {
                        let projected = projected_cam * node.transform;
                        let uniformed = matrix::decompose_matrix64_to_uniform(&projected);
                        let matrix_bytes = bytemuck::bytes_of(&uniformed);

                        let start = counter * self.pipeline.transforms.aligned_uniform_size;
                        let end = start + matrix_bytes.len();

                        self.pipeline.transforms.data[start..end].copy_from_slice(matrix_bytes);

                        counter += 1;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn input(&mut self, event: InputEvent) {
        self.input_state.process_input(&self.camera, event);
    }
}
