use cgmath::{
    num_traits::clamp, EuclideanSpace, InnerSpace, Point2, Point3, Quaternion, Rad, Rotation,
    Rotation3, Vector3, Zero,
};
use std::sync::{Arc, RwLock};

use crate::dynamics::{Camera, PositionState};

pub const WGS84_A: f64 = 6_378_137.0; // meters

// Tuning
const MIN_ALT_M: f64 = 10.0;
const MAX_ALT_M: f64 = 50_000_000.0;
const ANGULAR_SENS_RAD_PER_PX: f64 = 0.003;
const ZOOM_SENS_PER_TICK: f64 = 0.12;
const DAMPING_PER_SEC: f64 = 3.0;

#[derive(Debug, Clone)]
pub struct DynamicsState {
    pub position: PositionState,

    // Momentum
    pub yaw_pitch_vel: Vector3<f64>, // x=yaw(dot around up_ref), y=pitch(around east_ref), z unused
    pub zoom_vel: f64,

    // ENU frame anchor: radial up at the *last pull_pt* the user clicked.
    // If None, we derive from current eye radial each frame.
    pub up_ref_from_pull: Option<Vector3<f64>>,
}

#[derive(Debug)]
pub struct Dynamics {
    state: RwLock<DynamicsState>,
}

impl Dynamics {
    pub fn new(starting_pos: PositionState) -> Self {
        Self {
            state: RwLock::new(DynamicsState {
                position: starting_pos,
                yaw_pitch_vel: Vector3::zero(),
                zoom_vel: 0.0,
                up_ref_from_pull: None,
            }),
        }
    }

    /// Mouse/gesture pull: `delta` in pixels; +x right, +y down.
    /// `pull_pt` is the world point the mouse started on (ECEF), used ONLY to pick a local frame.
    /// Rotation is ALWAYS about the Earth's center.
    pub fn pull(&self, delta: Point2<f64>, pull_pt: Option<Point3<f64>>) {
        let mut s = self.state.write().unwrap();

        // Cache the ENU "up" for consistent feel during this drag/flick
        if let Some(p) = pull_pt {
            let up = safe_normalize(p.to_vec()).unwrap_or(Vector3::unit_z());
            s.up_ref_from_pull = Some(up);
        }

        // Inject angular momentum (flick). Screen up should pitch up => negate delta.y.
        let yaw = ANGULAR_SENS_RAD_PER_PX * delta.x;
        let pitch = -ANGULAR_SENS_RAD_PER_PX * delta.y;

        s.yaw_pitch_vel.x += yaw;
        s.yaw_pitch_vel.y += pitch;
    }

    /// Wheel/pinch zoom. Positive `delta` = zoom OUT (further), negative = zoom IN (closer).
    /// `focus` is optional world point under the cursor; used to bias target, but we keep orbiting about origin.
    pub fn zoom(&self, delta: f64, focus: Option<Point3<f64>>) {
        let mut s = self.state.write().unwrap();

        s.zoom_vel += ZOOM_SENS_PER_TICK * delta;

        // Bias target toward focus for "zoom to cursor" feel, without changing orbit center.
        if let Some(f) = focus {
            s.position.target = f;
        }
    }

    /// Integrate and publish to camera.
    pub fn update(&self, dt: &core::time::Duration, camera: &Arc<Camera>) {
        let mut s = self.state.write().unwrap();

        let dt_s = dt.as_secs_f64().max(1e-6);
        let decay = (-DAMPING_PER_SEC * dt_s).exp();

        let mut pos = s.position.clone();

        // ----- Build local frame for this step -----
        // Use cached "up" from last pull start if available; otherwise geocentric up from eye.
        let up_ref = s
            .up_ref_from_pull
            .or_else(|| safe_normalize(pos.eye.to_vec()))
            .unwrap_or(Vector3::unit_z());

        // Robust east/north spanning set
        let mut east = safe_normalize(Vector3::unit_z().cross(up_ref))
            .or_else(|| safe_normalize(Vector3::unit_x().cross(up_ref)))
            .unwrap_or(Vector3::unit_x());
        let north = up_ref.cross(east); // already orthonormal if inputs were

        // ----- Apply angular motion (about ORIGIN) -----
        let yaw = s.yaw_pitch_vel.x * dt_s;
        let pitch = s.yaw_pitch_vel.y * dt_s;

        if yaw.abs() > 0.0 || pitch.abs() > 0.0 {
            let q_yaw = Quaternion::from_axis_angle(up_ref, Rad(yaw));
            let q_pitch = Quaternion::from_axis_angle(east, Rad(pitch));
            let q = q_yaw * q_pitch;

            // Rotate vectors around origin (geocentric orbit)
            pos.eye = Point3::from_vec(q.rotate_vector(pos.eye.to_vec()));
            pos.target = Point3::from_vec(q.rotate_vector(pos.target.to_vec()));
        }

        // ----- Dolly (zoom), multiplicative for consistent feel -----
        if s.zoom_vel.abs() > 0.0 {
            let eye_vec = pos.eye.to_vec();
            let r = eye_vec.magnitude();
            let r_new = clamp(
                r * (s.zoom_vel * dt_s).exp(),
                WGS84_A + MIN_ALT_M,
                WGS84_A + MAX_ALT_M,
            );

            let eye_dir = safe_normalize(eye_vec).unwrap_or(Vector3::unit_z());
            pos.eye = Point3::from_vec(eye_dir * r_new);

            // Keep target "biased" but don’t drag it to origin;
            // optionally, you could ease target toward eye_dir * WGS84_A to emulate surface lock.
        }

        // ----- Safety & housekeeping -----
        // Clamp radius again (handles cases with no zoom input)
        let r = pos.eye.to_vec().magnitude();
        let r_clamped = clamp(r, WGS84_A + MIN_ALT_M, WGS84_A + MAX_ALT_M);
        if (r - r_clamped).abs() > 0.0 {
            let eye_dir = safe_normalize(pos.eye.to_vec()).unwrap_or(Vector3::unit_z());
            pos.eye = Point3::from_vec(eye_dir * r_clamped);
        }

        // Stable geocentric up from eye
        //pos.up = safe_normalize(pos.eye.to_vec()).unwrap_or(Vector3::unit_z());

        // Exponential damping
        s.yaw_pitch_vel *= decay;
        s.zoom_vel *= decay;

        // Publish
        s.position = pos.clone();
        camera.update_dynamic_state(&pos);
    }
}

// ---------- Utilities ----------
#[inline]
fn safe_normalize(v: Vector3<f64>) -> Option<Vector3<f64>> {
    let m = v.magnitude();
    if m.is_finite() && m > 0.0 {
        Some(v / m)
    } else {
        None
    }
}
