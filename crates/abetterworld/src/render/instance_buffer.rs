use crate::render::RenderFrame;
use cgmath::Matrix4;
use cgmath::Point3;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Debug)]
pub struct Instance3x4 {
    pub r0: [f32; 4],
    pub r1: [f32; 4],
    pub r2: [f32; 4],
}

impl Instance3x4 {
    fn build(m: Matrix4<f64>, offset: &Point3<f64>) -> Self {
        let r0 = [
            m.x.x as f32,
            m.y.x as f32,
            m.z.x as f32,
            (m.w.x - offset.x) as f32,
        ];
        let r1 = [
            m.x.y as f32,
            m.y.y as f32,
            m.z.y as f32,
            (m.w.y - offset.y) as f32,
        ];
        let r2 = [
            m.x.z as f32,
            m.y.z as f32,
            m.z.z as f32,
            (m.w.z - offset.z) as f32,
        ];
        Instance3x4 { r0, r1, r2 }
    }
}

#[derive(Clone, Debug)]
pub struct InstanceBuffer {
    pub buf: wgpu::Buffer,
    pub capacity: usize, // #instances
}

impl InstanceBuffer {
    pub fn new(device: &wgpu::Device, capacity: usize) -> Self {
        let size = (capacity * std::mem::size_of::<Instance3x4>()) as wgpu::BufferAddress;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances_3x4_storage"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self { buf, capacity }
    }

    pub fn ensure_capacity(&mut self, device: &wgpu::Device, needed: usize) -> bool {
        if needed <= self.capacity {
            return false;
        }
        log::info!(
            "Growing instance buffer from {} to {} instances",
            self.capacity,
            needed
        );
        let new_cap = needed.next_power_of_two().max(self.capacity * 3 / 2);
        *self = Self::new(device, new_cap);
        true
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn build_instances(frame: &RenderFrame, eye_pos: &Point3<f64>) -> Vec<Instance3x4> {
    use rayon::iter::IntoParallelRefIterator;
    use rayon::iter::ParallelIterator;

    // Parallel over tiles; within each tile, iterate batches and map to instances.
    frame
        .tiles
        .par_iter()
        .flat_map_iter(|tile| {
            tile.nodes.iter().map(move |b| {
                // Let the tile/state build the instance for this node.
                Instance3x4::build(b.transform, eye_pos)
            })
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
pub fn build_instances(frame: &RenderFrame, eye_pos: &Point3<f64>) -> Vec<Instance3x4> {
    frame
        .tiles
        .iter()
        .flat_map(|tile| {
            tile.nodes.iter().map(move |b| {
                // Let the tile/state build the instance for this node.
                Instance3x4::build(b.transform, eye_pos)
            })
        })
        .collect()
}

pub fn upload_instances(queue: &wgpu::Queue, ibuf: &InstanceBuffer, instances: &[Instance3x4]) {
    if instances.len() > ibuf.capacity {
        log::error!(
            "Instance buffer too small ({}), need {}",
            ibuf.capacity,
            instances.len()
        );
    } else if instances.len() > 0 {
        queue.write_buffer(&ibuf.buf, 0, bytemuck::cast_slice(instances));
    }
}
