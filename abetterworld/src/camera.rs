use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    RwLock,
};

use crate::{
    coord_utils::geodetic_to_ecef_z_up,
    matrix::{decompose_matrix64_to_uniform, extract_frustum_planes, Uniforms},
    volumes::BoundingVolume,
};
use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix, Matrix4, Point3, Quaternion, Rotation, Rotation3,
    SquareMatrix, Vector2, Vector3, Vector4, Zero,
};

const EARTH_MIN_RADIUS_M: f64 = 6_350_000.0; // Conservative, accounting for sea-level radius
const EARTH_RADIUS_M: f64 = 6_371_000.0;
const EARTH_MAX_TERRAIN_HEIGHT_M: f64 = 10_000.0; // Everest + buffer
const EARTH_OUTER_BOUND_M: f64 = EARTH_MIN_RADIUS_M + EARTH_MAX_TERRAIN_HEIGHT_M;

const NEAR_MIN: f64 = 0.1; // Never go below this to avoid depth precision issues
const NEAR_MAX: f64 = 10_000.0; // Upper limit for near to avoid blowing out near plane

#[derive(Debug, Clone)]
pub struct CameraRefinementData {
    pub position: Vector3<f64>,
    pub far: f64,
    pub fovy: Deg<f64>,
}
impl CameraRefinementData {
    fn default() -> CameraRefinementData {
        CameraRefinementData {
            position: Vector3::zero(),
            far: 0.0,
            fovy: Deg(45.0),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CameraDerivedMatrices {
    planes: [(Vector4<f64>, Vector3<f64>, f64); 6],
    proj_view_matrix: Matrix4<f64>,
    uniform: Uniforms,

    near: f64,
    far: f64,
}
impl CameraDerivedMatrices {
    fn default() -> CameraDerivedMatrices {
        CameraDerivedMatrices {
            planes: [(Vector4::zero(), Vector3::zero(), 0.0); 6],
            proj_view_matrix: Matrix4::identity(),
            uniform: Uniforms::default(),

            near: 0.0,
            far: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CameraUserPosition {
    fovy: Deg<f64>,
    aspect: f64,
    eye: Point3<f64>,
    target: Point3<f64>,
    up: Vector3<f64>,

    near: Option<f64>,
    far: Option<f64>,
}

#[derive(Debug)]
pub struct Camera {
    generation: AtomicU64,
    dirty: AtomicBool,

    user_state: RwLock<CameraUserPosition>,
    derived_state: RwLock<CameraDerivedMatrices>,
    paging_state: RwLock<CameraRefinementData>,
}

impl Camera {
    pub fn new(start: CameraUserPosition) -> Self {
        let cam = Camera {
            generation: AtomicU64::new(1),
            dirty: AtomicBool::new(true),
            user_state: RwLock::new(start),
            derived_state: RwLock::new(CameraDerivedMatrices::default()),
            paging_state: RwLock::new(CameraRefinementData::default()),
        };

        cam
    }

    pub fn height_above_terrain(&self) -> f64 {
        let user_state = self.user_state.read().unwrap();
        let cam_world = user_state.eye.to_vec();
        // Distance from camera to Earth's center
        let distance_to_center = cam_world.magnitude();
        // Height above terrain is distance to center minus Earth's radius
        distance_to_center - EARTH_RADIUS_M
    }

    pub fn eye(&self) -> Point3<f64> {
        self.user_state.read().unwrap().eye
    }

    pub fn eye_vector(&self) -> Vector3<f64> {
        self.user_state.read().unwrap().eye.to_vec()
    }

    /// move camera and target along camera-right (x) and camera-up (y) axes
    pub fn pan(&self, delta: Vector2<f64>) {
        let mut user_state = self.user_state.write().unwrap();
        let view_dir = (user_state.target - user_state.eye).normalize();
        let right = view_dir.cross(user_state.up).normalize();
        let up = user_state.up;
        let shift = right * delta.x + up * delta.y;
        user_state.eye += shift;
        user_state.target += shift;

        self.dirty.store(true, Ordering::Relaxed);
    }

    /// move the eye closer/further from the target along the view direction
    pub fn zoom(&self, amount: f64) {
        let mut user_state = self.user_state.write().unwrap();
        let view_dir = (user_state.target - user_state.eye).normalize();
        user_state.eye += view_dir * amount;

        self.dirty.store(true, Ordering::Relaxed);
    }

    /// rotate camera up/down around the camera-right axis
    pub fn tilt(&self, angle: Deg<f64>) {
        let mut user_state = self.user_state.write().unwrap();
        let view_vec = user_state.eye - user_state.target;
        let right = (user_state.target - user_state.eye)
            .normalize()
            .cross(user_state.up)
            .normalize();
        let q: Quaternion<f64> = Quaternion::from_axis_angle(right, angle);
        let new_view = q.rotate_vector(view_vec);
        user_state.eye = user_state.target + new_view;
        user_state.up = q.rotate_vector(user_state.up).normalize();

        self.dirty.store(true, Ordering::Relaxed);
    }

    /// rotate camera left/right around the world-up axis
    pub fn yaw(&self, angle: Deg<f64>) {
        let mut user_state = self.user_state.write().unwrap();
        let view_vec = user_state.eye - user_state.target;
        let q: Quaternion<f64> = Quaternion::from_axis_angle(user_state.up, angle);
        let new_view = q.rotate_vector(view_vec);
        user_state.eye = user_state.target + new_view;

        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn refinement_data(&self) -> CameraRefinementData {
        self.paging_state.read().unwrap().clone()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// internal: recompute cam_world and UBO
    pub fn update(&self, distance_to_geom: Option<f64>) -> Matrix4<f64> {
        if !self.dirty.load(Ordering::Relaxed) {
            return self.derived_state.read().unwrap().proj_view_matrix;
        }

        let user_state = self.user_state.read().unwrap();
        let mut derived_state = self.derived_state.write().unwrap();

        let cam_world = user_state.eye.to_vec();
        let d = cam_world.magnitude();

        let altitude = (d - EARTH_RADIUS_M).max(1.0);
        let max_distance = distance_to_geom.unwrap_or(altitude);

        // More aggressive scaling for space views
        let near_scale = if altitude > 50_000.0 { 0.5 } else { 0.25 };

        // Near plane scales with altitude for depth precision and no clipping
        derived_state.near = user_state
            .near
            .unwrap_or((max_distance * near_scale).clamp(NEAR_MIN, NEAR_MAX));
        derived_state.far = user_state.far.unwrap_or(d);

        let proj64 = cgmath::perspective(
            user_state.fovy,
            user_state.aspect,
            derived_state.near,
            derived_state.far,
        );
        let model_view_mat = Matrix4::look_at_rh(user_state.eye, user_state.target, user_state.up);
        derived_state.proj_view_matrix = proj64 * model_view_mat;
        derived_state.planes = extract_frustum_planes(&derived_state.proj_view_matrix);
        derived_state.uniform = decompose_matrix64_to_uniform(&derived_state.proj_view_matrix);

        self.generation.fetch_add(1, Ordering::Relaxed);
        self.dirty.store(false, Ordering::Relaxed);

        if let Ok(mut state) = self.paging_state.write() {
            *state = CameraRefinementData {
                position: cam_world,
                far: derived_state.far,
                fovy: user_state.fovy,
            };
        }

        derived_state.proj_view_matrix
    }

    /// expose the latest UBO
    pub fn uniform(&self) -> Uniforms {
        self.derived_state.read().unwrap().uniform
    }

    /// expose the latest planes
    pub fn planes(&self) -> [(Vector4<f64>, Vector3<f64>, f64); 6] {
        self.derived_state.read().unwrap().planes
    }

    pub fn frustum_corners(&self) -> [Point3<f64>; 8] {
        let user_state = self.user_state.read().unwrap();
        let derived_state = self.derived_state.read().unwrap();

        let fovy_rad = user_state.fovy.0.to_radians();
        let view_dir = (user_state.target - user_state.eye).normalize();
        let right = view_dir.cross(user_state.up).normalize();
        let up = user_state.up.normalize();

        // Height and width at near and far planes
        let tan_fovy = (fovy_rad / 2.0).tan();
        let near_height = 2.0 * tan_fovy * derived_state.near;
        let near_width = near_height * user_state.aspect;
        let far_height = 2.0 * tan_fovy * derived_state.far;
        let far_width = far_height * user_state.aspect;

        // Centers of near and far planes
        let near_center = user_state.eye + view_dir * derived_state.near;
        let far_center = user_state.eye + view_dir * derived_state.far;

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

    pub fn print_frustum_planes(&self) {
        println!("Frustum planes:");
        let labels = ["Left", "Right", "Bottom", "Top", "Near", "Far"];
        for (i, plane) in self.derived_state.read().unwrap().planes.iter().enumerate() {
            println!(
                "Plane {} ({}): offset= {:?}, normal = {:?}, d = {:?}",
                i, labels[i], plane.0, plane.1, plane.2,
            );
        }
    }
}

pub fn init_camera() -> (Camera, Camera) {
    let radius = 6_378_137.0;
    let distance: f64 = radius * 2.0;

    let eye = Point3::new(distance, 100.0, 100.0);
    let target = Point3::new(0.0, 0.0, 0.0);
    let up = Vector3::unit_z();
    let camera = Camera::new(CameraUserPosition {
        fovy: Deg(45.0),
        aspect: 1.0,
        eye,
        target,
        up,
        near: None,
        far: None,
    });
    camera.update(None);

    let debug_eye = geodetic_to_ecef_z_up(34.4208, -119.6982, 200.0);
    let debug_eye_pt: Point3<f64> = Point3::new(debug_eye.0, debug_eye.1, debug_eye.2);
    let debug_camera = Camera::new(CameraUserPosition {
        fovy: Deg(45.0),
        aspect: 1.0,
        eye: debug_eye_pt,
        target,
        up,
        near: None,
        far: None,
    });
    debug_camera.update(None);

    (camera, debug_camera)
}
