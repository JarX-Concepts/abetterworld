use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Matrix4, Point3, SquareMatrix, Vector3, Vector4, Zero,
};

pub type FrustumPlanes = [(Vector4<f64>, Vector3<f64>, f64); 6];
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, RwLock,
};

use crate::{
    dynamics::PositionState,
    helpers::{
        decompose_matrix64_to_uniform, extract_frustum_planes, geodetic_to_ecef_z_up,
        remove_translation, Uniforms,
    },
    Config,
};

pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

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
    planes: FrustumPlanes,
    proj_view: Matrix4<f64>,
    proj_view_inv: Matrix4<f64>,
    uniform: Uniforms,

    near: f64,
    far: f64,
}

impl CameraDerivedMatrices {
    fn default() -> CameraDerivedMatrices {
        CameraDerivedMatrices {
            planes: [(Vector4::zero(), Vector3::zero(), 0.0); 6],
            proj_view: Matrix4::identity(),
            proj_view_inv: Matrix4::identity(),
            uniform: Uniforms::default(),

            near: 0.0,
            far: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CameraUserPosition {
    position: PositionState,

    fovy: Deg<f64>,
    aspect: f64,
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

    pub fn position(&self) -> PositionState {
        self.user_state.read().unwrap().position.clone()
    }

    pub fn eye_vector(&self) -> Vector3<f64> {
        self.user_state.read().unwrap().position.eye.to_vec()
    }

    pub fn set_viewport(&self, width: f64, height: f64) {
        if let Ok(mut state) = self.user_state.write() {
            state.aspect = width as f64 / height as f64;
            state.position.viewport_wh = (width, height);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn update_dynamic_state(&self, new_state: &PositionState) {
        let mut updated_state = self.user_state.write().unwrap();
        // is it different
        if updated_state.position != *new_state {
            updated_state.position = new_state.clone();
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    pub fn refinement_data(&self) -> CameraRefinementData {
        self.paging_state.read().unwrap().clone()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// internal: recompute cam_world and UBO
    pub fn update(&self, distance_to_geom: Option<f64>) -> (Point3<f64>, Uniforms, bool) {
        if !self.dirty.load(Ordering::Relaxed) {
            return (
                self.user_state.read().unwrap().position.eye,
                self.derived_state.read().unwrap().uniform,
                false,
            );
        }

        let user_state = self.user_state.read().unwrap();
        let mut derived_state = self.derived_state.write().unwrap();

        let cam_world = user_state.position.eye.to_vec();
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

        // Projection (f64 to preserve precision while deriving planes, etc.)
        let proj64 = cgmath::perspective(
            user_state.fovy,
            user_state.aspect,
            derived_state.near,
            derived_state.far,
        );

        // View matrix (right-handed look-at). This is the *full* view: R^T * T(-eye).
        let view64 = Matrix4::look_at_rh(
            user_state.position.eye,
            user_state.position.target,
            user_state.position.up,
        );

        // 1) For CPU culling and world->clip math on CPU, keep the full P*V.
        let proj_view_full = proj64 * view64;
        derived_state.planes = extract_frustum_planes(&proj_view_full);
        derived_state.proj_view = proj_view_full;
        derived_state.proj_view_inv = proj_view_full.invert().unwrap_or(Matrix4::identity());

        // 2) For the GPU *camera uniform*, remove translation from V to avoid double-subtracting,
        //    because each node model will already be pretranslated by -eye on the CPU.
        let view_no_translation64 = remove_translation(view64); // ~R^T
        let proj_view_rot64 = proj64 * view_no_translation64; // P * R^T
        derived_state.uniform = decompose_matrix64_to_uniform(&proj_view_rot64);

        self.generation.fetch_add(1, Ordering::Relaxed);
        self.dirty.store(false, Ordering::Relaxed);

        if let Ok(mut state) = self.paging_state.write() {
            *state = CameraRefinementData {
                position: cam_world,
                far: derived_state.far,
                fovy: user_state.fovy,
            };
        }

        (user_state.position.eye, derived_state.uniform, true)
    }

    /// expose the latest UBO
    pub fn uniform(&self) -> Uniforms {
        self.derived_state.read().unwrap().uniform
    }

    /// expose the latest planes
    pub fn planes(&self) -> FrustumPlanes {
        self.derived_state.read().unwrap().planes
    }

    pub fn proj_view_inv(&self) -> Matrix4<f64> {
        self.derived_state.read().unwrap().proj_view_inv
    }

    pub fn frustum_corners(&self) -> [Point3<f64>; 8] {
        let user_state = self.user_state.read().unwrap();
        let derived_state = self.derived_state.read().unwrap();

        let fovy_rad = user_state.fovy.0.to_radians();
        let view_dir = (user_state.position.target - user_state.position.eye).normalize();
        let right = view_dir.cross(user_state.position.up).normalize();
        let up = user_state.position.up.normalize();

        // Height and width at near and far planes
        let tan_fovy = (fovy_rad / 2.0).tan();
        let near_height = 2.0 * tan_fovy * derived_state.near;
        let near_width = near_height * user_state.aspect;
        let far_height = 2.0 * tan_fovy * derived_state.far;
        let far_width = far_height * user_state.aspect;

        // Centers of near and far planes
        let near_center = user_state.position.eye + view_dir * derived_state.near;
        let far_center = user_state.position.eye + view_dir * derived_state.far;

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

pub fn init_camera(geodetic_pos: Point3<f64>) -> Camera {
    let main_eye = geodetic_to_ecef_z_up(geodetic_pos[0], geodetic_pos[1], geodetic_pos[2]);

    let eye = Point3::new(main_eye.0, main_eye.1, main_eye.2);
    let target = Point3::new(0.0, 0.0, 0.0);
    let up = Vector3::unit_z();
    let camera = Camera::new(CameraUserPosition {
        fovy: Deg(45.0),
        aspect: 1.0,
        position: PositionState {
            eye,
            target,
            up,
            viewport_wh: (0.0, 0.0),
        },
        near: None,
        far: None,
    });
    camera.update(None);

    camera
}

pub fn camera_config(abw_config: &Config) -> (Arc<Camera>, Option<Arc<Camera>>) {
    let camera = Arc::new(init_camera(Point3::new(
        abw_config.geodetic_position.0,
        abw_config.geodetic_position.1,
        abw_config.geodetic_position.2,
    )));

    let debug_camera_option = if abw_config.use_debug_camera {
        Some(Arc::new(init_camera(Point3::new(
            abw_config.debug_camera_geodetic_position.0,
            abw_config.debug_camera_geodetic_position.1,
            abw_config.debug_camera_geodetic_position.2,
        ))))
    } else {
        None
    };

    (camera, debug_camera_option)
}
