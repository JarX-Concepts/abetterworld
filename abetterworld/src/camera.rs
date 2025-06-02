use bytemuck::{Pod, Zeroable};
use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Point3, Quaternion, Rotation, Rotation3,
    SquareMatrix, Vector2, Vector3, Vector4, Zero,
};

use crate::{matrix::Uniforms, tiles::BoundingVolume};

const EARTH_RADIUS_M: f64 = 6_371_000.0;

#[derive(Debug, Clone)]
pub struct Camera {
    // intrinsic
    fovy: Deg<f64>,
    aspect: f64,
    // orientation
    eye: Point3<f64>,
    target: Point3<f64>,
    up: Vector3<f64>,

    // depth
    near: f64,
    far: f64,

    // derived
    uniform: Uniforms,
    cam_world: Vector3<f64>,
    planes: [(Vector4<f64>, Vector3<f64>, f64); 6],
}

impl Camera {
    pub fn new(
        fovy: Deg<f64>,
        aspect: f64,
        eye: Point3<f64>,
        target: Point3<f64>,
        up: Vector3<f64>,
    ) -> Self {
        let mut cam = Camera {
            fovy,
            aspect,
            eye,
            target,
            up: up.normalize(),
            uniform: Uniforms::from_eye_target(Matrix4::identity(), eye, target, up), // dummy
            planes: [(Vector4::zero(), Vector3::zero(), 0.0); 6],
            cam_world: Vector3::zero(),
            near: 0.0,
            far: 0.0,
        };
        cam.update(None);
        cam
    }

    pub fn height_above_terrain(&self) -> f64 {
        self.eye.to_vec().magnitude() - EARTH_RADIUS_M
    }

    /// move camera and target along camera-right (x) and camera-up (y) axes
    pub fn pan(&mut self, delta: Vector2<f64>) {
        let view_dir = (self.target - self.eye).normalize();
        let right = view_dir.cross(self.up).normalize();
        let up = self.up;
        let shift = right * delta.x + up * delta.y;
        self.eye += shift;
        self.target += shift;
    }

    /// move the eye closer/further from the target along the view direction
    pub fn zoom(&mut self, amount: f64) {
        let view_dir = (self.target - self.eye).normalize();
        self.eye += view_dir * amount;
    }

    /// rotate camera up/down around the camera-right axis
    pub fn tilt(&mut self, angle: Deg<f64>) {
        let view_vec = self.eye - self.target;
        let right = (self.target - self.eye)
            .normalize()
            .cross(self.up)
            .normalize();
        let q: Quaternion<f64> = Quaternion::from_axis_angle(right, angle);
        let new_view = q.rotate_vector(view_vec);
        self.eye = self.target + new_view;
        self.up = q.rotate_vector(self.up).normalize();
    }

    /// rotate camera left/right around the world-up axis
    pub fn yaw(&mut self, angle: Deg<f64>) {
        let view_vec = self.eye - self.target;
        let q: Quaternion<f64> = Quaternion::from_axis_angle(self.up, angle);
        let new_view = q.rotate_vector(view_vec);
        self.eye = self.target + new_view;
        // up stays the same
    }

    fn extract_frustum_planes(mat: &Matrix4<f64>) -> [(Vector4<f64>, Vector3<f64>, f64); 6] {
        let m = mat;
        let rows = [
            m.x, // row 0
            m.y, // row 1
            m.z, // row 2
            m.w, // row 3
        ];

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
            let n = Vector3::new(plane.0.x, plane.0.y, plane.0.z);
            let len = n.magnitude();
            plane.1 = n / len;
            plane.2 = plane.0.w / len;
        }

        planes
    }

    /// internal: recompute cam_world and UBO
    pub fn update(&mut self, far: Option<f64>) {
        // recompute world‐space camera position
        let view = Matrix4::look_at_rh(self.eye, self.target, self.up);
        let view_inv = view.invert().expect("view must invert");
        self.cam_world = view_inv.w.truncate();

        // set near/far as before
        let d = self.cam_world.magnitude();
        self.near = (d - (EARTH_RADIUS_M * 2.0)).max(1.0);
        self.far = far.unwrap_or(d);
        let proj64 = cgmath::perspective(self.fovy, self.aspect, self.near, self.far);

        // rebuild the split-offset UBO
        self.uniform = Uniforms::from_eye_target(proj64, self.eye, self.target, self.up);

        let mat_f64 = Matrix4::from(self.uniform.mat).cast::<f64>().unwrap();
        self.planes = Self::extract_frustum_planes(&mat_f64);
    }

    /// expose the latest UBO
    pub fn uniform(&self) -> Uniforms {
        self.uniform
    }

    pub fn is_bounding_volume_visible(&self, bv: &BoundingVolume) -> bool {
        let offset_vec = Vector3::new(
            self.uniform.offset[0] as f64,
            self.uniform.offset[1] as f64,
            self.uniform.offset[2] as f64,
        );
        let obb = bv.to_obb();

        // For each plane, do OBB vs. plane test
        let center = Vector3::new(obb.center.x, obb.center.y, obb.center.z);
        let half_axes: [Vector3<f64>; 3] = [
            Vector3::new(obb.half_axes[0].x, obb.half_axes[0].y, obb.half_axes[0].z),
            Vector3::new(obb.half_axes[1].x, obb.half_axes[1].y, obb.half_axes[1].z),
            Vector3::new(obb.half_axes[2].x, obb.half_axes[2].y, obb.half_axes[2].z),
        ];

        for &(_eqn, n, d) in &self.planes {
            // Project box center onto plane normal
            let dist = n.dot(center) + d;

            // Compute effective radius (extents) along plane normal
            let r = half_axes[0].magnitude() * n.dot(half_axes[0].normalize()).abs()
                + half_axes[1].magnitude() * n.dot(half_axes[1].normalize()).abs()
                + half_axes[2].magnitude() * n.dot(half_axes[2].normalize()).abs();

            if dist + r < 0.0 {
                // Completely outside this plane, so culled
                return false;
            }
        }

        // Passed all planes
        true
    }

    pub fn needs_refinement(
        &self,
        bounding_volume: &BoundingVolume,
        geometric_error: f64,
        screen_height: f64,
        sse_threshold: f64,
    ) -> bool {
        // 1. Frustum test
        /*         if !self.is_bounding_volume_visible(bounding_volume) {
            return false;
        } */

        if !geometric_error.is_finite() || geometric_error > 1e20 {
            return true; // Always refine root/sentinel
        }

        let obb = bounding_volume.to_obb();
        let cam_pos = self.cam_world;
        let closest_point = obb.closest_point(cam_pos);

        let is_inside = (closest_point - cam_pos).magnitude() < f64::EPSILON;
        let dist = if is_inside {
            0.0
        } else {
            let diagonal = obb.half_axes.iter().map(|a| a.magnitude()).sum::<f64>() * 2.0;
            (closest_point - cam_pos).magnitude().max(diagonal * 0.01)
        };

        log::info!(
            "bounding_volume: {:?}, camera world pos: {:?}",
            bounding_volume,
            cam_pos
        );
        log::info!("Obb: {:?}, closest point: {:?}", obb, closest_point);
        log::info!(
            "Offset Camera position: {:?}, distance: {:.2}m",
            cam_pos,
            dist
        );
        log::info!("Are we within {}m: {:?}", self.far, dist <= self.far);
        if dist > self.far {
            return false; // far away, no need to refine
        }

        // 3. Compute vertical FOV (in radians)
        let vertical_fov = self.fovy.0.to_radians();

        // 4. SSE formula
        let sse = (geometric_error * screen_height) / (dist * (vertical_fov * 0.5).tan() * 2.0);

        // 5. Needs refinement?
        sse > sse_threshold
    }

    pub fn frustum_corners(&self) -> [Point3<f64>; 8] {
        let fovy_rad = self.fovy.0.to_radians();
        let view_dir = (self.target - self.eye).normalize();
        let right = view_dir.cross(self.up).normalize();
        let up = self.up.normalize();

        // Height and width at near and far planes
        let tan_fovy = (fovy_rad / 2.0).tan();
        let near_height = 2.0 * tan_fovy * self.near;
        let near_width = near_height * self.aspect;
        let far_height = 2.0 * tan_fovy * self.far;
        let far_width = far_height * self.aspect;

        // Centers of near and far planes
        let near_center = self.eye + view_dir * self.near;
        let far_center = self.eye + view_dir * self.far;

        // 8 corners: [near_tl, near_tr, near_br, near_bl, far_tl, far_tr, far_br, far_bl]
        [
            // Near top-left
            near_center + up * (near_height / 2.0) - right * (near_width / 2.0),
            // Near top-right
            near_center + up * (near_height / 2.0) + right * (near_width / 2.0),
            // Near bottom-right
            near_center - up * (near_height / 2.0) + right * (near_width / 2.0),
            // Near bottom-left
            near_center - up * (near_height / 2.0) - right * (near_width / 2.0),
            // Far top-left
            far_center + up * (far_height / 2.0) - right * (far_width / 2.0),
            // Far top-right
            far_center + up * (far_height / 2.0) + right * (far_width / 2.0),
            // Far bottom-right
            far_center - up * (far_height / 2.0) + right * (far_width / 2.0),
            // Far bottom-left
            far_center - up * (far_height / 2.0) - right * (far_width / 2.0),
        ]
    }
}
