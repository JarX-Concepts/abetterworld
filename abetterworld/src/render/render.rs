use cgmath::Point3;

use crate::{
    content::{RenderableMap, RenderableState},
    helpers::{AbwError, Uniforms},
    render::{build_instances, upload_instances},
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
        draw_tile_volumes: Option<bool>,
        draw_debug_camera: Option<bool>,
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

        /*     if draw_tile_volumes.unwrap_or(false) {
            self.draw_all_tile_volumes(render_pass, queue);
        }

        if draw_debug_camera.unwrap_or(false) {
            self.draw_debug_camera(render_pass, queue);
        } */

        Ok(())
    }

    /* fn draw_all_tile_volumes(render_pass: &mut wgpu::RenderPass, queue: &wgpu::Queue, camera: &Arc<Camera>, pipeline: &RenderPipeline) {
        let camera_vp = world.camera.uniform();
        queue.write_buffer(
            &world.debug_pipeline.transforms.uniform_buffer,
            0,
            bytemuck::bytes_of(&camera_vp),
        );

        render_pass.set_bind_group(0, &world.debug_pipeline.transforms.uniform_bind_group, &[]);
        render_pass.set_pipeline(&world.debug_pipeline.pipeline);
        render_pass.set_index_buffer(
            world.frustum_render.volume_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );
        render_pass.set_vertex_buffer(0, world.frustum_render.vertex_buffer.slice(..));

        // start past the debug camera
        let mut volume_counter = 1;
        let latest_render = world.content.renderable.read().unwrap();
        for _tile in latest_render.iter() {
            if volume_counter >= MAX_VOLUMES {
                //eprintln!("Hit maximum number of volumes");
            } else {
                render_pass.draw_indexed(0..36, volume_counter as i32 * 8, 0..1);
            }
            volume_counter += 1;
        }
    }

    pub fn draw_debug_camera(&world, render_pass: &mut wgpu::RenderPass, queue: &wgpu::Queue) {
        let mut camera_vp = world.camera.uniform();
        camera_vp.free_space = 0.5;

        queue.write_buffer(
            &world.debug_pipeline.transforms.uniform_buffer,
            0,
            bytemuck::bytes_of(&camera_vp),
        );

        render_pass.set_bind_group(0, &world.debug_pipeline.transforms.uniform_bind_group, &[]);
        render_pass.set_pipeline(&world.debug_pipeline.pipeline);

        let corners = world.debug_camera.frustum_corners();
        let new_frustum_vertices: Vec<DebugVertex> = corners
            .iter()
            .map(|p| DebugVertex {
                position: [p.x as f32, p.y as f32, p.z as f32],
                color: [1.0, 0.0, 0.0, 1.0],
            })
            .collect();

        queue.write_buffer(
            &world.frustum_render.vertex_buffer,
            0,
            bytemuck::cast_slice(&new_frustum_vertices),
        );

        render_pass.set_vertex_buffer(0, world.frustum_render.vertex_buffer.slice(..));
        render_pass.set_index_buffer(
            world.frustum_render.frustum_buffer.slice(..),
            wgpu::IndexFormat::Uint16,
        );
        render_pass.draw_indexed(0..36, 0, 0..1);
    } */

    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,

        world: &mut WorldPrivate,
        eye_pos: &Point3<f64>,
        uniform_camera_mvp: &Uniforms,
        draw_tile_volumes: Option<bool>,
        draw_debug_camera: Option<bool>,
    ) -> Result<(), AbwError> {
        /*         if (draw_debug_camera) {
            let user_state = content.camera.user_state.read().unwrap();
            let derived_state = content.camera.derived_state.read().unwrap();

            // Update the camera refinement data
            if let Ok(mut state) = content.paging_state.write() {
                *state = CameraRefinementData {
                    position: user_state.position.eye,
                    far: derived_state.far,
                    fovy: user_state.fovy,
                };
            }

            // Calculate the minimum distance to the camera
            min_distance = (user_state.position.eye - user_state.position.target).magnitude();
        }

        if draw_tile_volumes {
            let latest_render = content.renderable.read().unwrap();
            let mut volume_counter = 1;
            for tile in latest_render.iter() {
                let renderable = &tile.1;

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
                        &frustum_render.vertex_buffer,
                        volume_counter * SIZE_OF_VOLUME,
                        bytemuck::cast_slice(&new_frustum_vertices),
                    );
                    volume_counter += 1;
                }
            }
        } */

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

        Ok(())
    }
}
