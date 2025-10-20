use cgmath::{
    num_traits::Float, Deg, EuclideanSpace, InnerSpace, Matrix, Matrix4, Point3, SquareMatrix,
    Vector3, Vector4, Zero,
};

pub type FrustumPlanes = [(Vector4<f64>, Vector3<f64>, f64); 5];
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, RwLock,
};

use crate::{
    dynamics::{proj_reverse_z_infinite_f64, proj_reverse_z_infinite_inv_f64},
    helpers::{
        decompose_matrix64_to_uniform, ecef_to_lla_wgs84, extract_frustum_planes_reverse_z,
        geodetic_to_ecef_z_up, remove_translation, Uniforms,
    },
    Config,
};

pub const EARTH_RADIUS_M: f64 = 6_371_000.0;

const NEAR_MIN: f64 = 0.1; // Never go below this to avoid depth precision issues
const NEAR_MAX: f64 = 10_000.0; // Upper limit for near to avoid blowing out near plane

#[derive(Debug, Clone)]
pub struct CameraRefinementData {
    pub position: Point3<f64>,
    pub far: f64,
    pub fovy: Deg<f64>,
    pub planes: FrustumPlanes,
    pub screen_height: f64,
    pub sse_threshold: f64,
}
impl CameraRefinementData {
    fn default() -> CameraRefinementData {
        CameraRefinementData {
            position: Point3::new(0.0, 0.0, 0.0),
            far: 0.0,
            fovy: Deg(45.0),
            screen_height: 1024.0,
            sse_threshold: 40.0,
            planes: [(Vector4::zero(), Vector3::zero(), 0.0); 5],
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
            planes: [(Vector4::zero(), Vector3::zero(), 0.0); 5],
            proj_view: Matrix4::identity(),
            proj_view_inv: Matrix4::identity(),
            uniform: Uniforms::default(),

            near: 0.0,
            far: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CameraDynamicsData {
    pub proj: Matrix4<f64>,
    pub proj_inv: Matrix4<f64>,
    pub view_inv: Matrix4<f64>,
    pub proj_view_inv: Matrix4<f64>,
    pub proj_view: Matrix4<f64>,
    pub eye: Point3<f64>,
    pub viewport_wh: (f64, f64),
    pub fov_y: Deg<f64>,
}

impl CameraDynamicsData {
    fn default() -> CameraDynamicsData {
        CameraDynamicsData {
            proj: Matrix4::identity(),
            proj_inv: Matrix4::identity(),
            view_inv: Matrix4::identity(),
            proj_view: Matrix4::identity(),
            proj_view_inv: Matrix4::identity(),
            eye: Point3::new(0.0, 0.0, 0.0),
            viewport_wh: (0.0, 0.0),
            fov_y: Deg(45.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub struct PositionState {
    pub eye: Point3<f64>,
    pub target: Point3<f64>,
    pub up: Vector3<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct CameraUserPosition {
    position: PositionState,

    viewport_wh: (f64, f64),
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
    dynamics_data: RwLock<CameraDynamicsData>,
}

impl Camera {
    pub fn new(start: CameraUserPosition) -> Self {
        let cam = Camera {
            generation: AtomicU64::new(1),
            dirty: AtomicBool::new(true),
            user_state: RwLock::new(start),
            derived_state: RwLock::new(CameraDerivedMatrices::default()),
            paging_state: RwLock::new(CameraRefinementData::default()),
            dynamics_data: RwLock::new(CameraDynamicsData::default()),
        };

        cam
    }

    pub fn position(&self) -> PositionState {
        self.user_state.read().unwrap().position.clone()
    }

    pub fn dynamics(&self) -> CameraDynamicsData {
        self.dynamics_data.read().unwrap().clone()
    }

    pub fn eye_vector(&self) -> Vector3<f64> {
        self.user_state.read().unwrap().position.eye.to_vec()
    }

    pub fn set_viewport(&self, width: f64, height: f64) {
        if let Ok(mut state) = self.user_state.write() {
            state.aspect = width as f64 / height as f64;
            state.viewport_wh = (width, height);
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn set_position(&self, new_state: &PositionState) {
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

    pub fn update(&self) -> (Point3<f64>, Uniforms, bool) {
        // Fast path
        if !self.dirty.load(Ordering::Acquire) {
            let eye = {
                let us = self.user_state.read().unwrap();
                us.position.eye
            };
            let uniform = {
                let ds = self.derived_state.read().unwrap();
                ds.uniform
            };
            return (eye, uniform, false);
        }

        // ---- 1) Snapshot user_state ----
        let (position, near_override, _far_override, fovy, aspect, viewport_wh) = {
            let us = self.user_state.read().unwrap();
            (
                us.position,
                us.near,
                us.far, // ignored for reverse-Z
                us.fovy,
                us.aspect,
                us.viewport_wh,
            )
        };
        let eye = position.eye;
        let target = position.target;
        let up = position.up;
        let (vw, vh) = viewport_wh;

        // ---- 2) Heavy math lock-free ----
        let (_lat, _lon, altitude) = ecef_to_lla_wgs84(eye);
        let near_scale = match altitude {
            a if a < 10_000.0 => 0.01,
            a if a < 100_000.0 => 10.0,
            _ => 100.0,
        };
        let near = near_override.unwrap_or(near_scale).max(1e-4);

        // Projection (reverse-Z, infinite far)
        let proj64 = proj_reverse_z_infinite_f64(fovy.into(), aspect, near);

        let view64 = Matrix4::look_at_rh(eye, target, up);

        // Quick orthonormality/handedness check on view rotation (R^T·R ≈ I)
        fn rot_orthonorm_residual(m: &Matrix4<f64>) -> f64 {
            use cgmath::{InnerSpace, Matrix3, SquareMatrix};
            // Extract rotation (upper-left 3x3 of view⁻¹ or equivalently remove translation from view)
            let r = Matrix3::new(
                m.x.x, m.x.y, m.x.z, m.y.x, m.y.y, m.y.z, m.z.x, m.z.y, m.z.z,
            );
            let i = Matrix3::<f64>::identity();
            let rt_r = r.transpose() * r;
            let d = rt_r - i;
            [
                d.x.x.abs(),
                d.x.y.abs(),
                d.x.z.abs(),
                d.y.x.abs(),
                d.y.y.abs(),
                d.y.z.abs(),
                d.z.x.abs(),
                d.z.y.abs(),
                d.z.z.abs(),
            ]
            .iter()
            .copied()
            .fold(0.0f64, f64::max)
        }
        let rot_res = rot_orthonorm_residual(&view64);

        // World->clip
        let proj_view_full = proj64 * view64;
        let planes = extract_frustum_planes_reverse_z(&proj_view_full);

        // Inverses
        let p_inv = proj_reverse_z_infinite_inv_f64(fovy.into(), aspect, near);
        let v_inv = view64.invert().unwrap_or(Matrix4::identity());
        let proj_view_inv = v_inv * p_inv;

        // Tiny helper: max ∞-norm of (A·B − I)
        fn inv_check(a: &Matrix4<f64>, b: &Matrix4<f64>) -> f64 {
            let ab = *a * *b;
            let i = Matrix4::<f64>::identity();
            let d = ab - i;
            let mut maxv = 0.0;
            for c in 0..4 {
                for r in 0..4 {
                    maxv = maxv.max(d[c][r].abs());
                }
            }
            maxv
        }
        let proj_inv_res = inv_check(&proj64, &p_inv);
        let view_inv_res = inv_check(&view64, &v_inv);
        let pv_inv_res = inv_check(&proj_view_full, &proj_view_inv);

        // GPU uniform (P * R^T)
        let view_no_translation64 = remove_translation(view64); // ~R^T
        let proj_view_rot64 = proj64 * view_no_translation64;
        let uniform = decompose_matrix64_to_uniform(&proj_view_rot64);

        // Consistency: proj_view_full vs (P * [R^T|0;0 1] * T) — we can’t compare directly,
        // but we can check that removing translation yields close to proj_view_rot64.
        let view_rot_only = remove_translation(view64);
        let pv_no_trans = proj64 * view_rot_only;
        let mut pv_rot_res = 0.0;
        for c in 0..4 {
            for r in 0..4 {
                pv_rot_res = pv_rot_res.max((pv_no_trans[c][r] - proj_view_rot64[c][r]).abs());
            }
        }

        // ---- 3) Commit derived_state ----
        {
            let mut ds = self.derived_state.write().unwrap();
            ds.near = near;
            ds.far = f64::INFINITY;
            ds.planes = planes;
            ds.uniform = uniform;
        }

        self.generation.fetch_add(1, Ordering::Relaxed);
        self.dirty.store(false, Ordering::Release);

        // ---- 4) Update aux states ----
        if let Ok(mut state) = self.paging_state.write() {
            *state = CameraRefinementData {
                position: eye,
                far: f64::INFINITY,
                fovy,
                screen_height: vh,
                planes,
                sse_threshold: state.sse_threshold, // keep existing
            };
        }

        if let Ok(mut state) = self.dynamics_data.write() {
            *state = CameraDynamicsData {
                eye,
                fov_y: fovy,
                proj: proj64,
                proj_inv: p_inv,
                view_inv: v_inv,
                proj_view: proj_view_full,
                proj_view_inv,
                viewport_wh,
            };
        }

        (eye, uniform, true)
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

    let eye = Point3::new(main_eye.x, main_eye.y, main_eye.z);
    let target = Point3::new(0.0, 0.0, 0.0);
    let up = Vector3::unit_z();
    let camera = Camera::new(CameraUserPosition {
        fovy: Deg(45.0),
        aspect: 1.0,
        position: PositionState { eye, target, up },
        near: None,
        far: None,
        viewport_wh: (1024.0, 768.0),
    });
    camera.update();

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
