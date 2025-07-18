use bytemuck::{Pod, Zeroable};
use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix, Matrix4, Point3, Quaternion, Rotation, Rotation3,
    SquareMatrix, Vector2, Vector3, Vector4, Zero,
};

use crate::{
    camera,
    coord_utils::geodetic_to_ecef_z_up,
    matrix::{decompose_matrix64_to_uniform, Uniforms},
    tiles::{BoundingVolume, OrientedBoundingBox},
};

const EARTH_RADIUS_M: f64 = 6_371_000.0;

#[derive(Debug, Clone)]
pub struct Camera {
    // intrinsic
    fovy: Deg<f64>,
    aspect: f64,
    // orientation
    pub eye: Point3<f64>,
    target: Point3<f64>,
    up: Vector3<f64>,

    // depth
    near: f64,
    far: f64,

    // derived
    uniform: Uniforms,
    cam_world: Vector3<f64>,
    planes: [(Vector4<f64>, Vector3<f64>, f64); 6],
    proj_view_matrix: Matrix4<f64>,
}

impl Camera {
    pub fn new(
        fovy: Deg<f64>,
        aspect: f64,
        eye: Point3<f64>,
        target: Point3<f64>,
        up: Vector3<f64>,
    ) -> Self {
        let cam = Camera {
            fovy,
            aspect,
            eye,
            target,
            up: up.normalize(),
            uniform: Uniforms::default(),
            planes: [(Vector4::zero(), Vector3::zero(), 0.0); 6],
            cam_world: eye.to_vec(),
            near: 0.0,
            far: 0.0,
            proj_view_matrix: Matrix4::identity(),
        };

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
        let m = mat; //mat.transpose();

        let rows = [
            m.row(0).to_owned(),
            m.row(1).to_owned(),
            m.row(2).to_owned(),
            m.row(3).to_owned(),
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
            let normal = Vector3::new(plane.0.x, plane.0.y, plane.0.z);
            let len = normal.magnitude();
            plane.1 = normal / len;
            plane.2 = plane.0.w / len;
        }

        //println!("Planes: {:?}", planes);

        planes
    }

    /// internal: recompute cam_world and UBO
    pub fn update(&mut self, far: Option<f64>) -> Matrix4<f64> {
        // set near/far as before
        self.cam_world = self.eye.to_vec();
        let d = self.cam_world.magnitude();
        self.near = (d - EARTH_RADIUS_M).abs().clamp(0.01, 5000.0);
        self.far = far.unwrap_or(d);

        let proj64 = cgmath::perspective(self.fovy, self.aspect, self.near, self.far);
        let model_view_mat = Matrix4::look_at_rh(self.eye, self.target, self.up);
        self.proj_view_matrix = proj64 * model_view_mat;
        self.planes = Self::extract_frustum_planes(&self.proj_view_matrix);
        self.uniform = decompose_matrix64_to_uniform(&self.proj_view_matrix);

        self.proj_view_matrix
    }

    /// expose the latest UBO
    pub fn uniform(&self) -> Uniforms {
        self.uniform
    }

    pub fn proj_view(&self) -> Matrix4<f64> {
        self.proj_view_matrix
    }

    pub fn print_frustum_planes(&self) {
        println!("Frustum planes:");
        let labels = ["Left", "Right", "Bottom", "Top", "Near", "Far"];
        for (i, plane) in self.planes.iter().enumerate() {
            println!(
                "Plane {} ({}): offset= {:?}, normal = {:?}, d = {:?}",
                i, labels[i], plane.0, plane.1, plane.2,
            );
        }
    }

    pub fn is_bounding_volume_visible(&self, bv: &BoundingVolume) -> bool {
        let aabb = bv.to_aabb();

        // Apply camera offset to the whole box
        //aabb.min -= offset;
        //aabb.max -= offset;Í

        for &(_, normal, d) in &self.planes {
            // p-vertex: most positive vertex in direction of normal
            let p = Vector3::new(
                if normal.x >= 0.0 {
                    aabb.max.x
                } else {
                    aabb.min.x
                },
                if normal.y >= 0.0 {
                    aabb.max.y
                } else {
                    aabb.min.y
                },
                if normal.z >= 0.0 {
                    aabb.max.z
                } else {
                    aabb.min.z
                },
            );

            if normal.dot(p) + d < 0.0 {
                //println!("False");
                return false;
            }
        }

        println!("Bounding Volume: {:?}", aabb);
        println!("True");

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

        if dist > self.far {
            //return false; // far away, no need to refine
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

        // Corner offsets
        let near_up = up * (near_height / 2.0);
        let near_right = right * (near_width / 2.0);
        let far_up = up * (far_height / 2.0);
        let far_right = right * (far_width / 2.0);

        [
            // Near plane (counter-clockwise from top-left when looking down -view_dir)
            near_center + near_up - near_right, // 0: near top-left
            near_center + near_up + near_right, // 1: near top-right
            near_center - near_up + near_right, // 2: near bottom-right
            near_center - near_up - near_right, // 3: near bottom-left
            // Far plane (counter-clockwise from top-left when looking down -view_dir)
            far_center + far_up - far_right, // 4: far top-left
            far_center + far_up + far_right, // 5: far top-right
            far_center - far_up + far_right, // 6: far bottom-right
            far_center - far_up - far_right, // 7: far bottom-left
        ]
    }
}

pub fn init_camera() -> (Camera, Camera) {
    let radius = 6_378_137.0;
    let distance: f64 = radius * 2.0;

    let eye = Point3::new(distance, 100.0, 100.0);
    let target = Point3::new(0.0, 0.0, 0.0);
    let up = Vector3::unit_z();
    let camera = Camera::new(Deg(45.0), 1.0, eye, target, up);

    let debug_eye = geodetic_to_ecef_z_up(34.4208, -119.6982, 20000.0);
    let debug_eye_pt: Point3<f64> = Point3::new(debug_eye.0, debug_eye.1, debug_eye.2);
    let mut debug_camera = Camera::new(Deg(45.0), 1.0, debug_eye_pt, target, up);
    debug_camera.update(None);

    (camera, debug_camera)
}
