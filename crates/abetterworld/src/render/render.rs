use cgmath::{EuclideanSpace, InnerSpace, Point3};

use crate::{
    content::{Ray, TileKey, MAX_RENDERABLE_TILES_US},
    dynamics::FrustumPlanes,
    helpers::{is_bounding_volume_visible, AbwError, Uniforms},
    render::{
        build_instances, get_renderable_tile, rebuild_tile_bg, upload_instances,
        with_renderable_state, DebugVertex, RenderableMap, SIZE_OF_VOLUME,
    },
    world::WorldPrivate,
};

pub struct RenderAndUpdate {
    frame: RenderFrame,
}

pub struct RenderFrame {
    pub tiles: Vec<TileKey>,
}

fn build_frame(
    renderables: &RenderableMap,
    tile_culling: bool,
    planes: FrustumPlanes,
) -> RenderFrame {
    // --- Phase 2: frontier traversal from roots ---
    let mut frame = RenderFrame { tiles: Vec::new() };

    for (_hash_key, renderable_tile) in renderables.iter() {
        let tile_guard = renderable_tile.read().unwrap();

        if let Some(_renderable) = &tile_guard.renderable_state {
            frame.tiles.push(tile_guard.key);
        }
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
        world: &WorldPrivate,
        draw_tile_volumes: bool,
        draw_debug_camera: bool,
    ) -> Result<(), AbwError> {
        render_pass.set_pipeline(&world.pipeline.pipeline);
        render_pass.set_bind_group(0, &world.pipeline.bindings.tile_bg, &[]);

        let mut node_counter: u32 = 0;
        let renderables = &world.content.renderable;
        for render_tile_id in self.frame.tiles.iter() {
            with_renderable_state(renderables, *render_tile_id, |render_tile| {
                for _node in render_tile.nodes.iter() {
                    for mesh in render_tile.meshes.iter() {
                        render_pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                        render_pass.set_index_buffer(
                            mesh.index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );

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
                    }
                    node_counter += 1;
                }
            })?;
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
            if index >= MAX_RENDERABLE_TILES_US {
                //log::warn!("Hit maximum number of volumes (Render)");
                break;
            } else {
                render_pass.draw_indexed(0..36, (index as i32 + 1) * 8, 0..1);
            }
        }
    }

    pub fn get_min_distance(&self, eye_pos: &Point3<f64>) -> Option<f64> {
        let mut min_distance: Option<f64> = None;

        /*         {
            let eye_pos_vec = eye_pos.to_vec();
            let neg_eye_dir = -eye_pos_vec.normalize();
            for renderable in self.frame.tiles.iter() {
                let distance = renderable.culling_volume.ray_intersect(&Ray {
                    origin: eye_pos_vec,
                    direction: neg_eye_dir,
                });

                if let Some(dist) = distance {
                    if dist < min_distance.unwrap_or(f64::MAX) {
                        min_distance = Some(dist);
                    }
                }
            }
        } */

        min_distance
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
        tile_culling: bool,
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
            let planes = if let Some(debug_camera) = &world.debug_camera {
                debug_camera.planes()
            } else {
                world.camera.planes()
            };

            let renderable_tiles = &world.content.renderable;
            self.frame = build_frame(renderable_tiles, tile_culling, planes);
        }
        {
            let renderable_instances =
                build_instances(&self.frame, eye_pos, &world.content.renderable);

            // Short-lived borrow to check/ensure capacity and capture new capacity
            let (resized, new_capacity) = {
                if let Some(instance_buffer) = world.pipeline.bindings.instance_buffer.as_mut() {
                    let resized =
                        instance_buffer.ensure_capacity(device, renderable_instances.len());
                    let capacity = instance_buffer.capacity;
                    (resized, capacity)
                } else {
                    return Err(AbwError::Internal(
                        "Missing instance buffer in pipeline bindings".to_string(),
                    ));
                }
            };

            if resized {
                log::info!("Resized instance buffer to {} instances", new_capacity);
                rebuild_tile_bg(device, &mut world.pipeline);
            }

            if let Some(instance_buffer) = world.pipeline.bindings.instance_buffer.as_ref() {
                upload_instances(queue, instance_buffer, &renderable_instances);
            } else {
                return Err(AbwError::Internal(
                    "Missing instance buffer in pipeline bindings".to_string(),
                ));
            }
        }

        /*         if draw_tile_volumes {
            let renderables = &world.content.renderable;
            for (index, renderable_tile_id) in self.frame.tiles.iter().enumerate() {
                let renderable = get_renderable_tile(renderables, *renderable_tile_id)?;

                if index + 1 >= MAX_RENDERABLE_TILES_US {
                    //log::warn!("Hit maximum number of volumes (Update)");
                    break;
                }
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

                queue.write_buffer(
                    &world.frustum_render.vertex_buffer,
                    (index as u64 + 1) * SIZE_OF_VOLUME,
                    bytemuck::cast_slice(&new_frustum_vertices),
                );
            }
        } */

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
