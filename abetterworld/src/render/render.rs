use cgmath::Point3;

use crate::{
    content::{DebugVertex, RenderableMap, RenderableState},
    helpers::{AbwError, Uniforms},
    render::{build_instances, upload_instances, MAX_VOLUMES, SIZE_OF_VOLUME},
    world::WorldPrivate,
};
use std::sync::Arc;

pub struct RenderAndUpdate {
    frame: RenderFrame,
}

struct RenderingBatch {
    mesh_index: usize,
    node_index: usize,
    global_node_index: usize,
    material_index: Option<usize>,
}

struct RenderingTile {
    tile: Arc<RenderableState>,
    batches: Vec<RenderingBatch>,
}

pub struct RenderFrame {
    pub tiles: Vec<Arc<RenderableState>>,
}

// Flatten to a stable list (ideally grouped by mesh/material)
fn build_frame(latest_render: &RenderableMap) -> RenderFrame {
    let mut frame = RenderFrame { tiles: Vec::new() };

    for (_, r) in latest_render.iter() {
        // is tile is not in view, continue

        frame.tiles.push(r.clone());
    }
    frame
}

impl RenderAndUpdate {
    pub fn new() -> Self {
        Self {
            frame: RenderFrame { tiles: Vec::new() },
        }
    }

    pub fn render(
        &self,
        render_pass: &mut wgpu::RenderPass,
        queue: &wgpu::Queue,
        world: &WorldPrivate,
        draw_tile_volumes: bool,
        draw_debug_camera: bool,
    ) -> Result<(), AbwError> {
        render_pass.set_pipeline(&world.pipeline.pipeline);
        render_pass.set_bind_group(0, &world.pipeline.bindings.tile_bg, &[]);

        let mut node_counter: u32 = 0;
        for render_tile in self.frame.tiles.iter() {
            for _node in render_tile.nodes.iter() {
                for mesh in render_tile.meshes.iter() {
                    render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    render_pass
                        .set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);

                    if let Some(material_index) = mesh.material_index {
                        let material = &render_tile.materials[material_index];
                        if let Some(texture_index) = material.base_color_texture_index {
                            let texture_resource = &render_tile.textures[texture_index];
                            render_pass.set_bind_group(1, &texture_resource.bind_group, &[]);
                        }
                    }

                    // Draw call for this mesh
                    render_pass.draw_indexed(
                        0..mesh.num_indices,
                        0,
                        node_counter..node_counter + 1,
                    );
                    node_counter += 1;
                }
            }
        }

        if draw_tile_volumes {
            self.draw_all_tile_volumes(render_pass, world);
        }

        if draw_debug_camera {
            self.draw_debug_camera(world, render_pass);
        }

        Ok(())
    }

    fn draw_all_tile_volumes(&self, render_pass: &mut wgpu::RenderPass, world: &WorldPrivate) {
        render_pass.set_bind_group(0, &world.debug_pipeline.bindings.tile_bg, &[]);
        render_pass.set_pipeline(&world.debug_pipeline.pipeline);
        render_pass.set_index_buffer(
            world.frustum_render.volume_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );
        render_pass.set_vertex_buffer(0, world.frustum_render.vertex_buffer.slice(..));

        for (index, _renderable) in self.frame.tiles.iter().enumerate() {
            if index >= MAX_VOLUMES as usize {
                log::warn!("Hit maximum number of volumes (Render)");
            } else {
                render_pass.draw_indexed(0..36, (index as i32 + 1) * 8, 0..1);
            }
        }
    }

    pub fn draw_debug_camera(&self, world: &WorldPrivate, render_pass: &mut wgpu::RenderPass) {
        render_pass.set_bind_group(0, &world.debug_pipeline.bindings.tile_bg, &[]);
        render_pass.set_pipeline(&world.debug_pipeline.pipeline);

        render_pass.set_vertex_buffer(0, world.frustum_render.vertex_buffer.slice(..));
        render_pass.set_index_buffer(
            world.frustum_render.frustum_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );
        render_pass.draw_indexed(0..36, 0, 0..1);
    }

    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,

        world: &mut WorldPrivate,
        eye_pos: &Point3<f64>,
        uniform_camera_mvp: &Uniforms,
        draw_tile_volumes: bool,
        draw_debug_camera: bool,
    ) -> Result<(), AbwError> {
        if draw_debug_camera {
            if let Some(debug_camera) = &world.debug_camera {
                let corners = debug_camera.frustum_corners();
                let new_frustum_vertices: Vec<DebugVertex> = corners
                    .iter()
                    .map(|p| DebugVertex {
                        position: [
                            (p.x - eye_pos.x) as f32,
                            (p.y - eye_pos.y) as f32,
                            (p.z - eye_pos.z) as f32,
                        ],
                        color: [1.0, 0.0, 0.0, 0.3],
                    })
                    .collect();

                queue.write_buffer(
                    &world.frustum_render.vertex_buffer,
                    0,
                    bytemuck::cast_slice(&new_frustum_vertices),
                );
            }
        }

        {
            if let Some(instance_buffer) = world.pipeline.bindings.instance_buffer.as_mut() {
                let renderable_tiles = world.content.renderable.read().unwrap();
                self.frame = build_frame(&renderable_tiles);
                let renderable_instances = build_instances(&self.frame, eye_pos);
                upload_instances(device, queue, instance_buffer, &renderable_instances);
            } else {
                return Err(AbwError::Internal(
                    "Missing instance buffer in pipeline bindings".to_string(),
                ));
            }
        }

        if draw_tile_volumes {
            for (index, renderable) in self.frame.tiles.iter().enumerate() {
                let new_frustum_vertices: Vec<DebugVertex> = renderable
                    .culling_volume
                    .corners
                    .iter()
                    .map(|p| DebugVertex {
                        position: [
                            (p.x - eye_pos.x) as f32,
                            (p.y - eye_pos.y) as f32,
                            (p.z - eye_pos.z) as f32,
                        ],
                        color: [1.0, 1.0, 0.25, 0.1],
                    })
                    .collect();

                if (index as u64) >= MAX_VOLUMES {
                    log::warn!("Hit maximum number of volumes (Update)");
                } else {
                    queue.write_buffer(
                        &world.frustum_render.vertex_buffer,
                        (index as u64 + 1) * SIZE_OF_VOLUME,
                        bytemuck::cast_slice(&new_frustum_vertices),
                    );
                }
            }
        }

        // main camera
        {
            let camera_buffer: &wgpu::Buffer = world
                .pipeline
                .bindings
                .camera_buffer
                .as_ref()
                .ok_or_else(|| AbwError::Internal("Missing camera buffer".to_string()))?;

            queue.write_buffer(
                &camera_buffer,
                0,
                bytemuck::cast_slice(std::slice::from_ref(uniform_camera_mvp)),
            );
        }

        // debug camera
        {
            if let Some(camera_buffer) = world.debug_pipeline.bindings.camera_buffer.as_ref() {
                queue.write_buffer(
                    camera_buffer,
                    0,
                    bytemuck::cast_slice(std::slice::from_ref(uniform_camera_mvp)),
                );
            }
        }

        Ok(())
    }
}
