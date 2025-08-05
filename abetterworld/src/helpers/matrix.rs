use bytemuck::{Pod, Zeroable};
use cgmath::{InnerSpace, Matrix, Matrix4, SquareMatrix, Vector3, Vector4, Zero};

use crate::content::BoundingBox;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Uniforms {
    pub mat: [[f32; 4]; 4], // 4x4 matrix in f32, with fractional translation
    pub offset: [f32; 3],   // integer world offset
    pub free_space: f32,    // padding for alignment
}

impl Uniforms {
    // default constructor for convenience
    pub fn default() -> Self {
        Self {
            mat: [[0.0; 4]; 4],
            offset: [0.0; 3],
            free_space: 0.0,
        }
    }
}

pub fn uniform_size(min_uniform_buffer_offset_alignment: usize) -> usize {
    let uniform_size = std::mem::size_of::<Uniforms>();

    fn align_to(value: usize, alignment: usize) -> usize {
        (value + alignment - 1) / alignment * alignment
    }
    align_to(uniform_size, min_uniform_buffer_offset_alignment)
}

/// Convert f64 Matrix4 to Uniforms (offset + f32 transform matrix)
pub fn decompose_matrix64_to_uniform(mat: &Matrix4<f64>) -> Uniforms {
    let translation = Vector3::new(mat.w.x, mat.w.y, mat.w.z);
    //let offset = translation.map(|v| v as f32);
    let offset = Vector3::new(0.0, 0.0, 0.0);

    // Subtract the coarse offset from the translation
    let offset_as_f64 = Vector3::new(offset.x as f64, offset.y as f64, offset.z as f64);
    let _remainder_translation = translation - offset_as_f64;

    // Convert the whole matrix to f32
    let mat32 = [
        [
            mat.x.x as f32,
            mat.x.y as f32,
            mat.x.z as f32,
            mat.x.w as f32,
        ],
        [
            mat.y.x as f32,
            mat.y.y as f32,
            mat.y.z as f32,
            mat.y.w as f32,
        ],
        [
            mat.z.x as f32,
            mat.z.y as f32,
            mat.z.z as f32,
            mat.z.w as f32,
        ],
        [
            mat.w.x as f32,
            mat.w.y as f32,
            mat.w.z as f32,
            mat.w.w as f32,
        ],
    ];

    Uniforms {
        mat: mat32,
        offset: [offset.x, offset.y, offset.z],
        free_space: 0.0,
    }
}

/// Convert Uniforms back to Matrix4<f64>
pub fn recompose_uniform_to_matrix64(uniforms: &Uniforms) -> Matrix4<f64> {
    let mut mat64 = Matrix4::<f64>::identity();
    for i in 0..4 {
        for j in 0..4 {
            mat64[i][j] = uniforms.mat[i][j] as f64;
        }
    }

    // Recompose the high-precision offset into translation (w row)
    mat64.w.x += uniforms.offset[0] as f64;
    mat64.w.y += uniforms.offset[1] as f64;
    mat64.w.z += uniforms.offset[2] as f64;

    mat64
}

pub fn extract_frustum_planes(mat: &Matrix4<f64>) -> [(Vector4<f64>, Vector3<f64>, f64); 6] {
    let rows = [mat.row(0), mat.row(1), mat.row(2), mat.row(3)];

    let mut planes = [(Vector4::zero(), Vector3::zero(), 0.0); 6];

    // Left
    planes[0].0 = rows[3] + rows[0];
    // Right
    planes[1].0 = rows[3] - rows[0];
    // Bottom
    planes[2].0 = rows[3] + rows[1];
    // Top
    planes[3].0 = rows[3] - rows[1];
    // Near
    planes[4].0 = rows[3] + rows[2];
    // Far
    planes[5].0 = rows[3] - rows[2];

    // Normalize planes and extract (normal, d)
    for plane in &mut planes {
        let normal = Vector3::new(plane.0.x, plane.0.y, plane.0.z);
        let len = normal.magnitude();
        plane.1 = normal / len;
        plane.2 = plane.0.w / len;
    }

    planes
}

pub fn is_bounding_volume_visible(
    planes: &[(Vector4<f64>, Vector3<f64>, f64); 6],
    bb: &BoundingBox,
) -> bool {
    for &(_, normal, d) in planes {
        let p = Vector3::new(
            if normal.x >= 0.0 { bb.max.x } else { bb.min.x },
            if normal.y >= 0.0 { bb.max.y } else { bb.min.y },
            if normal.z >= 0.0 { bb.max.z } else { bb.min.z },
        );

        let n = Vector3::new(
            if normal.x < 0.0 { bb.max.x } else { bb.min.x },
            if normal.y < 0.0 { bb.max.y } else { bb.min.y },
            if normal.z < 0.0 { bb.max.z } else { bb.min.z },
        );

        let dp_p = normal.dot(p) + d;
        let dp_n = normal.dot(n) + d;

        if dp_p < 0.0 {
            // Fully outside
            return false;
        }

        if dp_n < 0.0 {
            // Intersecting → run full corner test
            let all_outside = bb.corners.iter().all(|c| normal.dot(*c) + d < 0.0);
            if all_outside {
                return false;
            }
        }
    }

    true
}
