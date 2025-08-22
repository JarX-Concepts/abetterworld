use crate::dynamics::{Camera, EARTH_RADIUS_M};
use cgmath::{num_traits::Float, Deg, EuclideanSpace, InnerSpace, Point3, Rad, Vector3};
use std::sync::{Arc, RwLock};

const ROTATION_SENSITIVITY: f64 = 0.0000000008; // scaled by height
const PAN_SENSITIVITY: f64 = 0.00015; // tuned for GE‑like lateral feel
const ZOOM_SENSITIVITY: f64 = 0.0015; // scaled by height

// Damping parameters: critical-ish damping for quick settle without oscillation.
// v' = -2ζω v - ω^2 (x - x_eq)  (we use this form per-DOF; here x_eq is "no force")
const DAMP_ZETA: f64 = 1.2; // damping ratio
const NAT_FREQ: f64 = 3.0; // natural frequency (rad/s)

const MIN_HEIGHT: f64 = 5.0; // meters above terrain
const MAX_HEIGHT: f64 = 60_000_000.0; // ~60,000 km, beyond GEO for safety
const MIN_PITCH: f64 = Deg(-89.5).0; // looking almost straight down
const MAX_PITCH: f64 = Deg(-5.0).0; // prevent flipping over the top

#[derive(Debug, Clone, PartialEq)]
pub struct PositionState {
    pub eye: Point3<f64>,
    pub target: Point3<f64>,
    pub up: Vector3<f64>,
}

#[derive(Debug, Clone)]
struct OrbitParams {
    target: Point3<f64>,
    radius: f64, // distance from target
    yaw: f64,    // radians, 0 along +X in target-local frame
    pitch: f64,  // radians, negative = look down from above horizon
}

#[derive(Debug, Clone)]
struct OrbitVel {
    yaw: f64,
    pitch: f64,
    radius: f64,
    pan: Vector3<f64>, // world-space pan velocity applied to target
}

impl Default for OrbitVel {
    fn default() -> Self {
        OrbitVel {
            yaw: 0.0,
            pitch: 0.0,
            radius: 0.0,
            pan: Vector3::new(0.0, 0.0, 0.0),
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum Pivot {
    Center,  // orbit around (0,0,0)
    Surface, // orbit around target on globe
}

#[derive(Debug, Clone)]
pub struct DynamicsState {
    orbit: OrbitParams,
    vel: OrbitVel,
    pivot: Pivot,
    pub position: PositionState, // kept in sync for your camera API
}

#[derive(Debug)]
pub struct Dynamics {
    state: RwLock<DynamicsState>,
}

#[inline]
fn safe_normalize(v: Vector3<f64>, fallback: Vector3<f64>) -> Vector3<f64> {
    let m2 = v.magnitude2();
    if m2.is_finite() && m2 > 1e-20 {
        v / m2.sqrt()
    } else {
        fallback
    }
}

/// Build a stable ENU basis from a planet-normal `n` (unit).
/// Returns (east, north, up). Handles poles & degeneracy.
fn enu_from_normal(n: Vector3<f64>) -> (Vector3<f64>, Vector3<f64>, Vector3<f64>) {
    let up = safe_normalize(n, Vector3::unit_z());
    // choose a helper axis that's not collinear with up
    let helper = if up.x.abs() < 0.75 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    };
    let east = safe_normalize(up.cross(helper), Vector3::unit_y());
    let north = safe_normalize(up.cross(east), Vector3::unit_z());
    (east, north, up)
}

impl Dynamics {
    pub fn new(position: PositionState) -> Self {
        // Project/repair target if someone passed (0,0,0)
        let mut target = position.target;
        if !target.to_vec().magnitude2().is_finite() || target.to_vec().magnitude2() < 1e-20 {
            // derive a target from eye direction at Earth surface
            let dir = safe_normalize(position.eye.to_vec(), Vector3::unit_z());
            target = Point3::from_vec(dir * EARTH_RADIUS_M);
        }

        // Radius from target→eye
        let to_eye = position.eye - target;
        let radius = to_eye.magnitude().max(MIN_HEIGHT);

        // Globe up & ENU basis at target
        let up_world = safe_normalize(target.to_vec(), Vector3::unit_z());
        let (e, n, u) = enu_from_normal(up_world);

        // Forward is from eye to target
        let forward = safe_normalize(target - position.eye, -u);

        // Pitch: looking down is negative (match your convention)
        // forward·u = cos(angle to up), but we want "down from horizon":
        // pitch = asin(-forward·u), ∈ [-π/2, π/2]
        let dot_u = forward.dot(u).clamp(-1.0, 1.0);
        let pitch = (-dot_u).asin();

        // Yaw: use atan2 on the projection of forward onto the tangent plane (e,n)
        let tangential = safe_normalize(forward - dot_u * u, e); // if near zero, fall back to east
        let cos_y = tangential.dot(e).clamp(-1.0, 1.0);
        let sin_y = tangential.dot(n).clamp(-1.0, 1.0);
        let yaw = sin_y.atan2(cos_y); // [-π, π]

        let orbit = OrbitParams {
            target,
            radius,
            yaw,
            pitch,
        };
        let state = DynamicsState {
            orbit: orbit.clone(),
            vel: OrbitVel::default(),
            pivot: Pivot::Center, // NEW: default pivot is Earth center
            position,
        };

        let d = Self {
            state: RwLock::new(state),
        };
        d.rebuild_position_from_orbit();
        d
    }

    pub fn set_pivot_center(&self) {
        self.state.write().unwrap().pivot = Pivot::Center;
    }
    pub fn set_pivot_surface(&self) {
        self.state.write().unwrap().pivot = Pivot::Surface;
    }

    #[inline]
    fn pitch_limits(pivot: Pivot) -> (f64, f64) {
        match pivot {
            // Center pivot: allow full vertical sweep except near the poles
            Pivot::Center => (Deg(-89.5).0.to_radians(), Deg(89.5).0.to_radians()),
            // Surface pivot: keep horizon in view; avoid flipping over the top
            Pivot::Surface => (Deg(-89.5).0.to_radians(), Deg(-5.0).0.to_radians()),
        }
    }

    fn rebuild_position_from_orbit(&self) {
        let mut s = self.state.write().unwrap();
        let (target, radius, yaw, pitch, pivot) = {
            let o = &s.orbit;
            (o.target, o.radius, o.yaw, o.pitch, s.pivot)
        };

        match pivot {
            Pivot::Surface => {
                // --- your existing surface-pivot branch (unchanged, but using helpers if you added them) ---
                let up_world = (if target.to_vec().magnitude2() > 1e-20 {
                    target.to_vec()
                } else {
                    // fall back if someone set target to zero
                    Vector3::unit_z() * EARTH_RADIUS_M
                })
                .normalize();

                // ENU at target
                let n = up_world;
                let e = if (n.cross(Vector3::unit_x())).magnitude2() < 1e-12 {
                    Vector3::unit_y().cross(n).normalize()
                } else {
                    Vector3::unit_x().cross(n).normalize()
                };
                let u = n;
                let r = u.cross(e);

                let cy = yaw.cos();
                let sy = yaw.sin();
                let cp = pitch.cos();
                let sp = pitch.sin();
                let forward = (e * cy + r * sy) * cp + (-u) * sp;
                let fwd = if forward.magnitude2() > 1e-20 {
                    forward.normalize()
                } else {
                    -u
                };

                let eye = target - fwd * radius;

                s.position.eye = eye;
                s.position.target = target;
                s.position.up = up_world;
            }
            Pivot::Center => {
                let pivot_pt = Point3::new(0.0, 0.0, 0.0);

                // World axes
                let ex = Vector3::unit_x();
                let ez = Vector3::unit_z();

                // Eye direction from center
                let cy = yaw.cos();
                let sy = yaw.sin();
                let cp = pitch.cos();
                let sp = pitch.sin();
                let dir_from_center = (ex * cy + Vector3::unit_y() * sy) * cp + ez * sp;
                let dir = if dir_from_center.magnitude2() > 1e-20 {
                    dir_from_center.normalize()
                } else {
                    ez
                };

                // Eye position
                let eye = pivot_pt + dir * radius;

                // Forward toward center
                let forward = safe_normalize((pivot_pt - eye), -ez);

                // Pick an "up reference" close to world up, but make it ⟂ forward via Gram–Schmidt
                let mut up = ez - forward * forward.dot(ez);
                if up.magnitude2() < 1e-20 {
                    // forward ~ parallel to world-Z (at poles) → use X as reference
                    up = ex - forward * forward.dot(ex);
                }
                up = safe_normalize(up, Vector3::unit_y());

                // Re-orthonormalize to kill drift
                let right = safe_normalize(forward.cross(up), Vector3::unit_x());
                let up = safe_normalize(right.cross(forward), Vector3::unit_y());

                // Commit
                s.position.eye = eye;
                s.position.target = pivot_pt;
                s.position.up = up;

                // Keep orbit target pinned to center in this mode
                s.orbit.target = pivot_pt;
            }
        }
    }

    pub fn height_above_terrain(&self) -> f64 {
        let s = self.state.read().unwrap();
        let cam_world = s.position.eye.to_vec();
        cam_world.magnitude() - EARTH_RADIUS_M
    }

    /// Additive zoom impulse with altitude-aware sensitivity.
    /// `amount` is input delta; `in_flag` zooms toward the surface.
    pub fn zoom(&self, amount: f64, in_flag: bool) {
        // Snapshot radius once
        let radius = { self.state.read().unwrap().orbit.radius };
        let height = (radius - EARTH_RADIUS_M).clamp(MIN_HEIGHT, MAX_HEIGHT);

        // Altitude-aware gain: rises with height, fades near the ground.
        // h / (h + Hs) ∈ (0,1). Pick Hs to set where it "feels normal".
        const ZOOM_GAIN_BASE: f64 = 0.3; // overall sensitivity (try 0.14–0.22)
        const H_SAT: f64 = 50_000.0; // ~50 km: above this, you get near full gain

        let altitude_factor = height / (height + H_SAT);
        let gain = ZOOM_GAIN_BASE * altitude_factor;

        // Direction: zoom in reduces radius
        let dir = if in_flag { -1.0 } else { 1.0 };

        // Proposed step (in meters)
        let raw_step = dir * gain * amount * height;

        // Hard cap: at most a fraction of current height per impulse
        const MAX_ZOOM_FRAC: f64 = 0.5; // ≤50% of current height per call
        let max_step = height * MAX_ZOOM_FRAC;
        let step = raw_step.clamp(-max_step, max_step);

        // Apply as velocity so update() eases it out
        let mut s = self.state.write().unwrap();
        s.vel.radius += step;
    }

    /// Additive tilt impulse (momentum) around camera-right axis (affects pitch).
    pub fn tilt(&self, amount: f64, up_flag: bool) {
        let height = self.height_above_terrain().clamp(MIN_HEIGHT, MAX_HEIGHT);
        let impulse = amount.abs() * height * ROTATION_SENSITIVITY;
        let sign = if up_flag { -1.0 } else { 1.0 };
        let mut s = self.state.write().unwrap();
        s.vel.pitch += sign * impulse;
    }

    /// Additive yaw impulse (momentum) around world/globe up at target.
    pub fn yaw(&self, amount: f64, left_flag: bool) {
        let height = self.height_above_terrain().clamp(MIN_HEIGHT, MAX_HEIGHT);
        let impulse = amount.abs() * height * ROTATION_SENSITIVITY;
        let sign = if left_flag { 1.0 } else { -1.0 }; // left increases yaw
        let mut s = self.state.write().unwrap();
        s.vel.yaw += sign * impulse;
    }

    /// Pan the target laterally across the globe surface (adds momentum).
    /// dx,dy are in normalized screen units (e.g., pixels scaled by viewport).
    pub fn pan(&self, dx: f64, dy: f64) {
        let mut s = self.state.write().unwrap();
        match s.pivot {
            Pivot::Surface => {
                // your existing world-space pan velocity
                let height = self.height_above_terrain().clamp(MIN_HEIGHT, MAX_HEIGHT);
                let up_world = s.orbit.target.to_vec().normalize();
                let east = if (up_world.cross(Vector3::unit_x())).magnitude2() < 1e-12 {
                    Vector3::unit_y().cross(up_world).normalize()
                } else {
                    Vector3::unit_x().cross(up_world).normalize()
                };
                let north = up_world.cross(east).normalize();
                let v = (east * dx + north * dy) * (height * PAN_SENSITIVITY);
                s.vel.pan += v;
            }
            Pivot::Center => {
                // interpret dx,dy as angular velocities on orbit angles
                let ang_gain = 0.8 * ROTATION_SENSITIVITY * s.orbit.radius; // scale by radius for consistent feel
                s.vel.yaw += dx * ang_gain; // drag right = increase yaw
                s.vel.pitch += -dy * ang_gain; // drag up = tilt down toward ground
            }
        }
    }

    /// Optionally expose a GE-like "fly_to" that sets velocities toward a new target/radius.
    pub fn fly_to(&self, new_target: Point3<f64>, new_height: f64) {
        let mut s = self.state.write().unwrap();

        let new_radius = (EARTH_RADIUS_M + new_height)
            .clamp(EARTH_RADIUS_M + MIN_HEIGHT, EARTH_RADIUS_M + MAX_HEIGHT);

        // Take snapshots first so those immutable borrows end before we mutate s.vel.*
        let current_target = s.orbit.target;
        let current_radius = s.orbit.radius;

        let pan_impulse = (new_target - current_target) * 1.0; // coarse gain; update() damps it
        let radius_impulse = (new_radius - current_radius) * 0.75;

        s.vel.pan += pan_impulse;
        s.vel.radius += radius_impulse;
    }

    /// Gesture pinch (two-finger). Positive velocity or scale>1 => zoom in.
    pub fn gesture_pinch(&self, begin: bool, scale: f64, velocity: f64) {
        // Stabilize momentum at gesture start
        if begin {
            let mut s = self.state.write().unwrap();
            s.vel.radius *= 0.25; // keep a little inertia, but tame carry-in
            return;
        }

        // Convert scale to a small signed amount using ln
        // scale ~1.0 => near zero; >1 in => negative direction for radius
        let ln_scale = if scale.is_finite() && scale > 0.0 {
            scale.ln()
        } else {
            0.0
        };

        // Combine geometric scale + platform velocity for a responsive feel
        // velocity is device-specific; clamp to reasonable bounds.
        let v = velocity.clamp(-4.0, 4.0);

        // Normalize to our zoom "amount" units
        let amount = (ln_scale * 12.0) + (v * 0.30);

        // in_flag = zoom toward surface
        let in_flag = ln_scale > 0.0 || v > 0.0;
        self.zoom(amount.abs(), in_flag);
    }

    /// Two-finger orbit (e.g., Apple "orbit"/"pan-with-anchor"). Maps to yaw/pitch momentum.
    pub fn gesture_orbit(&self, begin: bool, dx: f64, dy: f64, vx: f64, vy: f64) {
        if begin {
            // damp angular carry-in so new gesture feels crisp
            let mut s = self.state.write().unwrap();
            s.vel.yaw *= 0.35;
            s.vel.pitch *= 0.35;
            return;
        }

        // Use movement distance primarily; velocity adds extra "throw"
        let boost_x = (dx * 1.0) + (vx * 0.12);
        let boost_y = (dy * 1.0) + (vy * 0.12);

        // Match mouse semantics: +dx to the right -> turn left_flag=false? (we used left_flag for sign)
        // Our mouse path called: yaw(delta_x.abs(), delta_x < 0.0) and tilt(delta_y.abs(), delta_y < 0.0).
        self.yaw(boost_x.abs(), boost_x < 0.0);
        self.tilt(boost_y.abs(), boost_y < 0.0);
    }

    /// Two-finger translate (pan). Reuses your pan() so it gets ENU behavior on Surface pivot.
    pub fn gesture_translate(&self, begin: bool, dx: f64, dy: f64, vx: f64, vy: f64) {
        if begin {
            let mut s = self.state.write().unwrap();
            s.vel.pan *= 0.35;
            return;
        }

        // Feed both displacement and a small velocity term to create momentum
        let k_disp = 1.0;
        let k_vel = 0.10;
        let gx = dx * k_disp + vx * k_vel;
        let gy = dy * k_disp + vy * k_vel;

        // Same screen-normalized units as mouse drag -> pan()
        self.pan(gx, gy);
    }

    /// Two-finger rotate (twist). We interpret as heading change (yaw), not roll.
    pub fn gesture_rotate(&self, begin: bool, radians: f64, velocity: f64) {
        if begin {
            let mut s = self.state.write().unwrap();
            s.vel.yaw *= 0.35;
            return;
        }

        // Positive radians usually means clockwise depending on platform; pick a consistent feel:
        // Treat positive radians as "turn right" -> decrease yaw (left_flag=false)
        // Add velocity to allow a nice throw.
        let v = velocity.clamp(-6.0, 6.0);
        let delta = radians + v * 0.06;

        self.yaw(delta.abs() * 600.0, /* left_flag = */ delta < 0.0);
    }

    /// Double-tap: quick zoom-in impulse (GE-like nudge). If you later have hit-testing,
    /// you can set the pivot/target toward the tapped location first.
    pub fn gesture_double_tap(&self, _x: f64, _y: f64) {
        // Slightly altitude-dependent shove inward
        let h = self.height_above_terrain().clamp(MIN_HEIGHT, MAX_HEIGHT);
        // Translate to our "amount" units in zoom(); aim for ~20-30% height change
        let amount = (h * 0.25 / (h + 50_000.0)).max(0.10) * 8.0;
        self.zoom(amount, true);
    }

    /// Touch down/up. You might swap pivots or reset minor state here if desired.
    pub fn gesture_touch_down(&self, active: bool, _x: f64, _y: f64) {
        let mut s = self.state.write().unwrap();
        if active {
            // when a multi-touch sequence starts, lightly damp all velocities
            s.vel.yaw *= 0.5;
            s.vel.pitch *= 0.5;
            s.vel.radius *= 0.5;
            s.vel.pan *= 0.5;
        } else {
            // on release, keep momentum as-is (nice "throw"); feel free to tweak
        }
    }

    /// GE-style smooth update: integrates velocities with damping, clamps, rebuilds eye/up.
    pub fn update(&self, dt: &core::time::Duration, camera: &Arc<Camera>) {
        let dt = dt.as_secs_f64().max(1e-6);
        let two_zeta_omega = 2.0 * DAMP_ZETA * NAT_FREQ;

        let mut s = self.state.write().unwrap();

        // angular velocity damping
        s.vel.yaw += -two_zeta_omega * s.vel.yaw * dt;
        s.vel.pitch += -two_zeta_omega * s.vel.pitch * dt;

        // integrate angles
        s.orbit.yaw += s.vel.yaw * dt;
        s.orbit.pitch += s.vel.pitch * dt;

        // ✅ clamp pitch based on pivot mode
        let (min_pitch, max_pitch) = Self::pitch_limits(s.pivot);
        s.orbit.pitch = s.orbit.pitch.clamp(min_pitch, max_pitch);

        // --- Integrate radius (zoom) with damping on velocity ---
        let radius_damp = 2.0 * DAMP_ZETA * NAT_FREQ * 0.8; // 0.8 -> slightly less damping for zoom
        s.vel.radius += -radius_damp * s.vel.radius * dt;

        s.orbit.radius = (s.orbit.radius + s.vel.radius * dt)
            .clamp(EARTH_RADIUS_M + MIN_HEIGHT, EARTH_RADIUS_M + MAX_HEIGHT);

        // --- Integrate pan in world, then project target back to globe surface ---
        let decay = 1.0 - two_zeta_omega * dt;
        let new_pan = s.vel.pan * decay;
        s.vel.pan = new_pan;
        let mut new_target = s.orbit.target + s.vel.pan * dt;

        // Reproject to the Earth surface (ECEF sphere model here; replace if you have terrain)
        let r = new_target.to_vec().magnitude().max(EARTH_RADIUS_M);
        new_target = Point3::from_vec(new_target.to_vec().normalize() * r);

        s.orbit.target = new_target;

        drop(s); // release lock before rebuilding to avoid re-entrancy surprises
        self.rebuild_position_from_orbit();

        // Push to render-side camera
        let pos = self.state.read().unwrap().position.clone();

        camera.update_dynamic_state(&pos);
    }
}
