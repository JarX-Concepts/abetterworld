use bytemuck::{Pod, Zeroable};
use cgmath::{EuclideanSpace, Matrix, Matrix4, Point3, SquareMatrix, Vector3, Vector4};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Uniforms {
    pub mat: [[f32; 4]; 4], // 4x4 matrix in f32, with fractional translation
    pub offset: [f32; 3],   // integer world offset
    pub _padding: f32,      // padding for alignment
}

impl Uniforms {
    pub fn build_from_3dtileset(mat64: Matrix4<f64>) -> Self {
        // Extract translation vector from the 4th column (elements [3][0..2])
        let translation = [mat64[3][0], mat64[3][1], mat64[3][2]];

        // Calculate integer offset by flooring each component
        let offset_i64 = [
            translation[0].floor(),
            translation[1].floor(),
            translation[2].floor(),
        ];

        // Create fractional translation remainder by subtracting offset
        let fractional_translation = [
            translation[0] - offset_i64[0],
            translation[1] - offset_i64[1],
            translation[2] - offset_i64[2],
        ];

        // Create a new f32 array by casting each element
        let mut mat_f32 = [[0.0f32; 4]; 4];

        mat_f32[3][0] = fractional_translation[0] as f32;
        mat_f32[3][1] = fractional_translation[1] as f32;
        mat_f32[3][2] = fractional_translation[2] as f32;
        mat_f32[3][3] = mat64[3][3] as f32;

        for row in 0..3 {
            for col in 0..4 {
                mat_f32[row][col] = mat64[row][col] as f32;
            }
        }

        // Convert integer offset to f32
        let offset_f32 = [
            offset_i64[0] as f32,
            offset_i64[1] as f32,
            offset_i64[2] as f32,
        ];

        Self {
            mat: mat_f32,
            offset: offset_f32,
            _padding: 0.0,
        }
    }

    /// Build a split-offset UBO from eye/target/up + proj.
    pub fn from_eye_target(
        proj: Matrix4<f64>,
        eye: Point3<f64>,
        target: Point3<f64>,
        up: Vector3<f64>,
    ) -> Self {
        // 1) Reconstruct full view and invert to get world-space camera pos:
        let view = Matrix4::look_at_rh(eye, target, up);
        let view_inv = view.invert().expect("view must be invertible");
        let cam_world: Vector3<f64> = view_inv.w.truncate();

        // 2) Compute world offset + fractional part:
        let world_offset = cam_world.map(f64::floor);
        let frac_world = cam_world - world_offset;

        // 3) Rebuild a fractional view by shifting eye & target down by world_offset:
        let eye_frac = Point3::from_vec(frac_world);
        let target_frac = Point3::new(
            target.x - world_offset.x,
            target.y - world_offset.y,
            target.z - world_offset.z,
        );
        let view_frac = Matrix4::look_at_rh(eye_frac, target_frac, up);

        // 4) Combine with projection in f32:
        let proj32 = proj.cast::<f32>().unwrap();
        let view32 = view_frac.cast::<f32>().unwrap();
        let vp32 = proj32 * view32;

        Uniforms {
            mat: vp32.into(),
            offset: [
                world_offset.x as f32,
                world_offset.y as f32,
                world_offset.z as f32,
            ],
            _padding: 0.0,
        }
    }

    pub fn project_point_test(&self, point: Vector3<f32>) {
        let offset_point =
            (point - Vector3::new(self.offset[0], self.offset[1], self.offset[2])).extend(1.0);
        let projected_point_f32 = Matrix4::from(self.mat) * offset_point;
        println!("projected_point_f32: {:?}", projected_point_f32);
    }
}

/// Converts a 4x4 ECEF transformation matrix from Z-up to Y-up.
pub fn z_up_to_y_up(mat: Matrix4<f64>) -> Matrix4<f64> {
    let mut rows = [
        mat.row(0).clone(),
        mat.row(1).clone(),
        mat.row(2).clone(),
        mat.row(3).clone(),
    ];

    // Swap Y and Z axes (row-wise): row 1 ↔ row 2
    rows.swap(1, 2);

    // Also swap translation components (4th column)
    let mut translation = Vector4::new(rows[0].w, rows[1].w, rows[2].w, rows[3].w);
    translation = Vector4::new(translation.x, translation.z, translation.y, translation.w);

    // Build new matrix
    Matrix4::new(
        rows[0].x,
        rows[0].y,
        rows[0].z,
        translation.x,
        rows[1].x,
        rows[1].y,
        rows[1].z,
        translation.y,
        rows[2].x,
        rows[2].y,
        rows[2].z,
        translation.z,
        rows[3].x,
        rows[3].y,
        rows[3].z,
        translation.w,
    )
}
