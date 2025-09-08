use cgmath::{EuclideanSpace, InnerSpace, Point3, Vector3, Zero};
use std::sync::{Arc, RwLock};

use crate::dynamics::{Camera, PositionState};

pub const EARTH_RADIUS_M: f64 = 6_378_137.0;
const MIN_HEIGHT_M: f64 = 10000.0;
const MAX_HEIGHT_M: f64 = 50_000_000.0;

const DAMPING_PER_SEC: f64 = 3.0;
const ZOOM_IMPULSE: f64 = 0.1;

#[derive(Debug, Clone)]
pub struct DynamicsState {
    pub position: PositionState,

    // Simple linear velocities for “momentum”
    pub eye_vel: Vector3<f64>,
    pub target_vel: Vector3<f64>,
}

#[derive(Debug)]
pub struct Dynamics {
    state: RwLock<DynamicsState>,
}

// ------------------ Helpers ------------------

fn clamp_height(eye: Point3<f64>) -> Point3<f64> {
    let r = eye.to_vec().magnitude();
    if r.is_finite() && r > 0.0 {
        let min_r = EARTH_RADIUS_M + MIN_HEIGHT_M;
        let max_r = EARTH_RADIUS_M + MAX_HEIGHT_M;
        let clamped_r = r.max(min_r).min(max_r);
        // project back to the same direction with clamped radius
        let dir = eye.to_vec() / r;
        Point3::from_vec(dir * clamped_r)
    } else {
        // fallback if eye is degenerate
        Point3::new(0.0, 0.0, EARTH_RADIUS_M + 1000.0)
    }
}

fn safe_normalize(v: Vector3<f64>) -> Vector3<f64> {
    let m = v.magnitude();
    if m > 1e-12 {
        v / m
    } else {
        Vector3::zero()
    }
}

fn altitude_of(eye: Point3<f64>) -> f64 {
    (eye.to_vec().magnitude() - EARTH_RADIUS_M).max(1.0)
}

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[inline]
fn smoothstep01(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Altitude-aware zoom step:
/// - Very gentle near the ground
/// - Grows smoothly up to space
fn zoom_step_for_alt(alt_m: f64) -> f64 {
    // Transition the sensitivity over the first ~20 km.
    // Below that, keep it gentle; above that, accelerate.
    let transition_alt = 20_000.0; // 20 km
    let t = smoothstep01(alt_m / transition_alt);

    // Near ground multiplier ~0.005 * alt (0.5 m at 100 m alt),
    // in space multiplier ~0.9 * alt
    let mult = lerp(0.005, 0.9, t);

    // Let it go tiny near ground, but keep a sane cap in space
    let min_step = 0.05; // 5 cm minimum
    let max_step = 2_000_000.0; // 2,000 km maximum
    (alt_m * mult).clamp(min_step, max_step)
}

impl Dynamics {
    pub fn new(starting_pos: PositionState) -> Self {
        Self {
            state: RwLock::new(DynamicsState {
                position: starting_pos,
                eye_vel: Vector3::zero(),
                target_vel: Vector3::zero(),
            }),
        }
    }

    pub fn zoom_to(&self, delta: f64, focus: Option<Point3<f64>>) {
        if delta.abs() < f64::EPSILON {
            return;
        }

        let mut s = self.state.write().expect("Dynamics write lock");

        let eye = s.position.eye;
        let target = s.position.target;
        let view_vec = target - eye;
        let view_dir = safe_normalize(view_vec);

        let alt = altitude_of(eye);

        // Non-linear altitude-aware base step
        let base_step = zoom_step_for_alt(alt);

        // Keep ln response for big wheel/touch deltas
        let signed_strength = delta.signum() * (1.0 + delta.abs()).ln();

        // delta>0 means "zoom in": dolly toward focus/target
        let dolly_dist = signed_strength * base_step;

        match focus {
            Some(focus_pt) => {
                let to_focus = focus_pt - eye;
                let dist_to_focus = to_focus.magnitude().max(1e-6);
                let dir_to_focus = to_focus / dist_to_focus;

                // Eye impulse
                s.eye_vel += dir_to_focus * (dolly_dist * ZOOM_IMPULSE);

                // Keep the focus under cursor, but be gentler near ground:
                // cap how aggressively we drag the target.
                // This prevents "snapping" when very close to terrain.
                let max_shift = 0.5; // was 0.9; smaller = less target tug
                let shift_ratio = (dolly_dist / dist_to_focus).clamp(-max_shift, max_shift);
                let target_shift = (focus_pt - target) * shift_ratio;
                s.target_vel += target_shift * ZOOM_IMPULSE;
            }
            None => {
                let dir = if view_dir.magnitude2() > 0.0 {
                    view_dir
                } else {
                    safe_normalize(eye.to_vec())
                };
                s.eye_vel += dir * (dolly_dist * ZOOM_IMPULSE);
            }
        }
    }

    /// Step the dynamics forward by `dt` and push to the camera.
    pub fn update(&self, dt: &core::time::Duration, camera: &Arc<Camera>) {
        let mut s = self.state.write().expect("Dynamics write lock");

        let dt_s = dt.as_secs_f64().max(1e-6);

        // Exponential damping (critically-damped-ish single-pole)
        let decay = (-DAMPING_PER_SEC * dt_s).exp();
        s.eye_vel *= decay;
        s.target_vel *= decay;

        // Take local copies of the velocities so we don't create overlapping borrows
        let eye_vel = s.eye_vel.clone();
        let target_vel = s.target_vel.clone();

        // Integrate
        s.position.eye += eye_vel * dt_s;
        s.position.target += target_vel * dt_s;

        // Keep the eye above the surface and within sane bounds
        s.position.eye = clamp_height(s.position.eye);

        // Recompute "up" as local geocentric up (matches GE feel well)
        let up_dir = safe_normalize(s.position.eye.to_vec());
        if up_dir.magnitude2() > 0.0 {
            //s.position.up = up_dir;
        }

        // Keep the eye above the surface and within sane bounds
        s.position.eye = clamp_height(s.position.eye);

        // Prevent pathological eye==target; nudge target forward if too close
        let eye_to_target = s.position.target - s.position.eye;
        if eye_to_target.magnitude() < 0.1 {
            s.position.target = s.position.eye + up_dir * 0.1;
        }

        println!(
            "Dynamics: eye={:?} alt={:.1}km target={:?} eye_vel={:?} target_vel={:?}",
            s.position.eye,
            altitude_of(s.position.eye) / 1000.0,
            s.position.target,
            s.eye_vel,
            s.target_vel
        );

        // Hand the camera the latest position
        camera.update_dynamic_state(&s.position);
    }
}
