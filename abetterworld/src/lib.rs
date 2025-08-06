mod cache;
mod content;
mod decode;
mod helpers;
mod render;

#[cfg(test)]
mod tests;

use cgmath::InnerSpace;
use decode::init;
use std::{
    ops::Deref,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    cache::init_tileset_cache,
    content::{start_pager, DebugVertex, Ray, Tile, TileManager},
    helpers::{
        channel::{channel, Receiver, Sender},
        is_bounding_volume_visible, matrix, AbwError,
    },
    render::{
        build_debug_pipeline, build_depth_buffer, build_frustum_render, build_pipeline,
        init_camera, input, Camera, FrustumRender, RenderPipeline, MAX_VOLUMES, SIZE_OF_VOLUME,
    },
};

const MAX_NEW_TILES_PER_FRAME: usize = 4;

pub struct ABetterWorld {
    pipeline: RenderPipeline,
    debug_pipeline: RenderPipeline,
    camera: Arc<Camera>,
    debug_camera: Arc<Camera>,
    depth_view: wgpu::TextureView,
    content: Arc<TileManager>,
    sender: Sender<Tile>,
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

impl ABetterWorld {
    /// Creates a new ABetterWorld.
    pub fn new(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> Self {
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

        let (loader_tx, render_rx) = channel::<Tile>(MAX_NEW_TILES_PER_FRAME * 2);

        let _ = init();
        let _ = start_pager(
            debug_camera_source.clone(),
            tile_content.clone(),
            loader_tx.clone(),
        );

        Self {
            pipeline,
            debug_pipeline,
            camera: camera_source,
            debug_camera: debug_camera_source,
            depth_view: depth.view,
            content: tile_content,
            input_state: input::InputState::new(),
            frustum_render,
            sender: loader_tx,
            receiver: render_rx,
        }
    }

    pub fn get_depth_view(&self) -> &wgpu::TextureView {
        &self.depth_view
    }

    pub fn render(
        &self,
        render_pass: &mut wgpu::RenderPass,
        queue: &wgpu::Queue,
        _device: &wgpu::Device,
    ) {
        {
            let latest_render = self.content.renderable.read().unwrap();
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
                    let renderable = &tile.1;
                    let nodes = &renderable.nodes;
                    let meshes = &renderable.meshes;
                    let textures = &renderable.textures;
                    let materials = &renderable.materials;
                    let _unload = &renderable.unload;
                    let culling_volume = &renderable.culling_volume;

                    let render_it = is_bounding_volume_visible(&planes, &culling_volume);

                    if !render_it {
                        counter += nodes.len() as u32;
                        continue;
                    }

                    for (_i, node) in nodes.iter().enumerate() {
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

        self.draw_all_tile_volumes(render_pass, queue);

        self.draw_debug_camera(render_pass, queue);
    }

    fn draw_all_tile_volumes(&self, render_pass: &mut wgpu::RenderPass, queue: &wgpu::Queue) {
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
        let latest_render = self.content.renderable.read().unwrap();
        for _tile in latest_render.iter() {
            if volume_counter >= MAX_VOLUMES {
                //eprintln!("Hit maximum number of volumes");
            } else {
                render_pass.draw_indexed(0..36, volume_counter as i32 * 8, 0..1);
            }
            volume_counter += 1;
        }
    }

    pub fn draw_debug_camera(&self, render_pass: &mut wgpu::RenderPass, queue: &wgpu::Queue) {
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

    pub fn update(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), AbwError> {
        self.debug_camera.deref().update(None);

        const BUDGET: Duration = Duration::from_millis(20);

        if let Some(layout) = self.pipeline.texture_bind_group_layout.as_ref() {
            self.content.unload_tiles();

            #[cfg(target_arch = "wasm32")]
            {
                let mut current_num_tiles = 0;

                // Pull tiles until either the channel is empty or we run out of time.
                while current_num_tiles < MAX_NEW_TILES_PER_FRAME {
                    current_num_tiles += 1;

                    match self.receiver.try_recv() {
                        Ok(mut tile) => {
                            use crate::content::tiles;
                            match tiles::content_render_setup(device, queue, layout, &mut tile) {
                                Ok(renderable_state) => {
                                    self.content.add_renderable(renderable_state);
                                }
                                Err(e) => {
                                    log::error!("Failed to set up tile for rendering: {e}");
                                    continue;
                                }
                            }
                        }
                        Err(_) => break, // nothing left
                    }
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let deadline = Instant::now() + BUDGET;
                // Pull tiles until either the channel is empty or we run out of time.
                while Instant::now() < deadline {
                    match self.receiver.try_recv() {
                        Ok(mut tile) => {
                            use crate::content::tiles;

                            match tiles::content_render_setup(device, queue, layout, &mut tile) {
                                Ok(renderable_state) => {
                                    self.content.add_renderable(renderable_state);
                                }
                                Err(e) => {
                                    log::error!("Failed to set up tile for rendering: {e}");
                                    continue;
                                }
                            }
                        }
                        Err(_) => break, // nothing left
                    }
                }
            }
        }

        //self.debug_camera.yaw(Deg(0.1));
        //self.debug_camera.write().unwrap().zoom(-500.0);
        self.debug_camera.update(None);

        let mut min_distance = f64::MAX;
        {
            let latest_render = self.content.renderable.read().unwrap();
            let camera_pos = self.camera.eye_vector();
            // start past the debug camera
            let mut volume_counter = 1;

            for tile in latest_render.iter() {
                let renderable = &tile.1;
                let distance = renderable.culling_volume.ray_intersect(&Ray {
                    origin: camera_pos,
                    direction: -camera_pos.normalize(),
                });

                if let Some(dist) = distance {
                    if dist < min_distance {
                        min_distance = distance.unwrap();
                    }
                }

                let new_frustum_vertices: Vec<DebugVertex> = renderable
                    .culling_volume
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
        let projected_cam = self.camera.update(Some(min_distance));

        {
            let latest_render = self.content.renderable.read().unwrap();

            let mut counter = 0;
            for tile in latest_render.iter() {
                let renderable = &tile.1;
                for (_i, node) in renderable.nodes.iter().enumerate() {
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

        Ok(())
    }

    pub fn input(&mut self, event: InputEvent) {
        self.input_state.process_input(&self.camera, event);
    }
}
