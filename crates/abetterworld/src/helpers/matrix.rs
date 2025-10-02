use bytemuck::{Pod, Zeroable};
use cgmath::{InnerSpace, Matrix, Matrix4, SquareMatrix, Vector3, Vector4, Zero};

use crate::content::BoundingBox;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Uniforms {
    pub mat: [[f32; 4]; 4], // 4x4 matrix in f32, with fractional translation
}

impl Uniforms {
    // default constructor for convenience
    pub fn default() -> Self {
        Self { mat: [[0.0; 4]; 4] }
    }
}

pub fn uniform_size(min_uniform_buffer_offset_alignment: usize) -> usize {
    let uniform_size = std::mem::size_of::<Uniforms>();

    fn align_to(value: usize, alignment: usize) -> usize {
        (value + alignment - 1) / alignment * alignment
    }
    align_to(uniform_size, min_uniform_buffer_offset_alignment)
}

pub fn decompose_matrix64_to_uniform(mat: &Matrix4<f64>) -> Uniforms {
    #[inline]
    fn f32_cast(x: f64) -> f32 {
        let y = x as f32;
        debug_assert!(x.is_finite(), "non-finite in uniform matrix");
        y
    }

    let mat32 = [
        [
            f32_cast(mat.x.x),
            f32_cast(mat.x.y),
            f32_cast(mat.x.z),
            f32_cast(mat.x.w),
        ], // col 0
        [
            f32_cast(mat.y.x),
            f32_cast(mat.y.y),
            f32_cast(mat.y.z),
            f32_cast(mat.y.w),
        ], // col 1
        [
            f32_cast(mat.z.x),
            f32_cast(mat.z.y),
            f32_cast(mat.z.z),
            f32_cast(mat.z.w),
        ], // col 2
        [
            f32_cast(mat.w.x),
            f32_cast(mat.w.y),
            f32_cast(mat.w.z),
            f32_cast(mat.w.w),
        ], // col 3
    ];

    Uniforms { mat: mat32 }
}

/// Convert Uniforms back to Matrix4<f64>
pub fn recompose_uniform_to_matrix64(uniforms: &Uniforms) -> Matrix4<f64> {
    let mut mat64 = Matrix4::<f64>::identity();
    for i in 0..4 {
        for j in 0..4 {
            mat64[i][j] = uniforms.mat[i][j] as f64;
        }
    }

    mat64
}

pub fn extract_frustum_planes_reverse_z(
    mat: &Matrix4<f64>,
) -> [(Vector4<f64>, Vector3<f64>, f64); 5] {
    let rows = [mat.row(0), mat.row(1), mat.row(2), mat.row(3)];

    let mut planes = [(Vector4::zero(), Vector3::zero(), 0.0); 5];

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

    // Normalize planes and extract (normal, d)
    for plane in &mut planes {
        let normal = Vector3::new(plane.0.x, plane.0.y, plane.0.z);
        let len = normal.magnitude();
        if len > 0.0 {
            plane.1 = normal / len;
            plane.2 = plane.0.w / len;
        }
    }

    planes
}

pub fn is_bounding_volume_visible(
    planes: &[(Vector4<f64>, Vector3<f64>, f64); 5], // L,R,B,T,Near (no Far)
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
            return false; // fully outside this plane
        }
        if dp_n < 0.0 {
            // Intersecting → run corner test for certainty
            let all_outside = bb.corners.iter().all(|c| normal.dot(*c) + d < 0.0);
            if all_outside {
                return false;
            }
        }
    }
    true
}

/// Zero out the translation of a column-major Matrix4<f64>.
#[inline]
pub fn remove_translation(mut v: Matrix4<f64>) -> Matrix4<f64> {
    // cgmath Matrix4 is column-major: x, y, z, w are columns.
    // Translation lives in w.x/y/z. Keep w.w = 1.
    v.w = Vector4::new(0.0, 0.0, 0.0, v.w.w);
    v
}
