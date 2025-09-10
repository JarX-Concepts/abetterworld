use crate::dynamics::{
    view_ray_from_screen_with_pose, world_to_screen, world_to_screen_proj, Camera,
    CameraDynamicsData, PositionState, ScreenPosition,
};
use cgmath::{
    EuclideanSpace, InnerSpace, Matrix4, One, Point2, Point3, Quaternion, Rad, Rotation, Rotation3,
    Vector3,
};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct DynamicsState {
    pub position: PositionState,
    pub rotation_pt: Point3<f64>,
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
                rotation_pt: Point3::new(0.0, 0.0, 0.0),
            }),
        }
    }

    pub fn zoom(
        &self,
        dynamics_data: &CameraDynamicsData,
        amount: f64,                     // +in / -out
        lock_pt: Option<ScreenPosition>, // pixel + optional world hit
    ) {
        use cgmath::{Basis3, Matrix3};

        // ---- Tunables ----
        const ZOOM_SENS: f64 = 0.00025; // zoom gain
        const MIN_SURFACE_RADIUS: f64 = 6_356_752.0 + 3.0; // ~WGS84_B + a tiny standoff
        const MIN_EYE_RADIUS: f64 = MIN_SURFACE_RADIUS + 0.5;
        const MAX_EYE_RADIUS: f64 = 50_000_000.0;
        const ITER: usize = 2; // Gauss-Newton iterations per zoom tick
        const EPS: f64 = 1e-4; // finite-diff step (radians)
        const ANGLE_CLAMP: f64 = 0.05; // max correction per iteration (rad)

        let center = Point3::new(0.0, 0.0, 0.0);
        let mut s = self.state.write().expect("Dynamics write lock");

        // Keep target at center by design.
        s.position.target = center;

        // --- 1) Altitude-scaled radius change (exponential for scale invariance)
        let eye_vec = s.position.eye - center;
        let r_now = eye_vec.magnitude().max(MIN_EYE_RADIUS);
        let dir_now = eye_vec / r_now;

        let altitude = (r_now - MIN_SURFACE_RADIUS).max(0.0);
        let altitude_gain = (1.0 + (altitude / 50_000.0)).ln().clamp(0.1, 1.6);

        let r_next = (r_now * (-amount * ZOOM_SENS * altitude_gain).exp())
            .clamp(MIN_EYE_RADIUS, MAX_EYE_RADIUS);

        let mut eye = center + dir_now * r_next;
        let mut up = s.position.up.normalize();

        // --- 2) If we have a lock point, screen-space correct via small rotations about center
        if let Some(sp) = lock_pt {
            if let Some(p_lock) = sp.world_position {
                // Small helper: apply a rotation (axis in world, angle in rad) about the *center*
                let mut apply_center_rot =
                    |axis: Vector3<f64>, angle: f64, eye_in: Point3<f64>, up_in: Vector3<f64>| {
                        if axis.magnitude2() == 0.0 || angle.abs() < 1e-12 {
                            (eye_in, up_in)
                        } else {
                            let axis_n = axis.normalize();
                            let q = Quaternion::from_axis_angle(axis_n, Rad(angle));
                            let eye_rel = eye_in - center;
                            let eye_out = center + q.rotate_vector(eye_rel);
                            let up_out = q.rotate_vector(up_in).normalize();
                            (eye_out, up_out)
                        }
                    };

                // Build a temporary pose for projection
                let mut pose = s.position.clone();

                for _ in 0..ITER {
                    // Project lock point with current (eye, up), target fixed at center
                    pose.eye = eye;
                    pose.target = center;
                    pose.up = up;

                    // Full view
                    let view64 = Matrix4::look_at_rh(pose.eye, Point3::new(0.0, 0.0, 0.0), pose.up);
                    let proj_view_full = dynamics_data.proj * view64;

                    if let Some(pix) =
                        world_to_screen_proj(p_lock, dynamics_data.viewport_wh, &proj_view_full)
                    {
                        // Pixel error: desired - current
                        let err = sp.xy - pix;

                        // Early exit if already tight
                        if err.magnitude2() < 0.25 {
                            // < 0.5 px^2
                            break;
                        }

                        // Build tangent basis at current eye direction (geocentric)
                        let view_dir = (center - eye).normalize(); // pointing from eye toward center
                                                                   // Prefer a stable lateral axis orthogonal to up
                        let u = view_dir.cross(up);
                        let u = if u.magnitude2() > 0.0 {
                            u.normalize()
                        } else {
                            // Degenerate: pick any orthogonal
                            let alt = view_dir.cross(Vector3::new(1.0, 0.0, 0.0));
                            if alt.magnitude2() > 0.0 {
                                alt.normalize()
                            } else {
                                view_dir.cross(Vector3::new(0.0, 1.0, 0.0)).normalize()
                            }
                        };
                        let v = u.cross(view_dir).normalize(); // completes right-handed basis on tangent plane

                        // Finite-difference Jacobian J: how pixels move for tiny rotations about u and v
                        let mut probe = |axis: Vector3<f64>| -> Option<Point2<f64>> {
                            let (eye_p, up_p) = apply_center_rot(axis, EPS, eye, up);
                            let mut pose_p = pose.clone();
                            pose_p.eye = eye_p;
                            pose_p.up = up_p;

                            // Full view
                            let view64 = Matrix4::look_at_rh(
                                pose_p.eye,
                                Point3::new(0.0, 0.0, 0.0),
                                pose_p.up,
                            );
                            let proj_view_full = dynamics_data.proj * view64;

                            world_to_screen_proj(p_lock, dynamics_data.viewport_wh, &proj_view_full)
                        };

                        let pix0 = pix;
                        let pix_u = match probe(u) {
                            Some(p) => p,
                            None => break,
                        };
                        let pix_v = match probe(v) {
                            Some(p) => p,
                            None => break,
                        };

                        // Columns of J are the per-axis pixel deltas / EPS
                        let j_col_u = (pix_u - pix0) * (1.0 / EPS);
                        let j_col_v = (pix_v - pix0) * (1.0 / EPS);

                        // Solve J * [du dv]^T = err  (2x2)
                        let a = j_col_u.x;
                        let b = j_col_v.x;
                        let c = j_col_u.y;
                        let d = j_col_v.y;
                        let det = a * d - b * c;

                        if det.abs() < 1e-12 {
                            // Ill-conditioned; take a tiny step toward geodesic to lock
                            let to_lock = (p_lock - eye).normalize();
                            let want = to_lock.cross(view_dir); // axis that would yaw/pitch toward lock
                            let angle = (err.magnitude() * 0.0005).clamp(-ANGLE_CLAMP, ANGLE_CLAMP);
                            let (eye2, up2) = apply_center_rot(want, angle, eye, up);
                            eye = eye2;
                            up = up2;
                            continue;
                        }

                        let inv = (1.0 / det) * Matrix3::new(d, -b, 0.0, -c, a, 0.0, 0.0, 0.0, 1.0);
                        // Multiply inv * [err.x, err.y, 0]
                        let du = inv.x.x * err.x + inv.x.y * err.y;
                        let dv = inv.y.x * err.x + inv.y.y * err.y;

                        // Clamp correction
                        let du = du.clamp(-ANGLE_CLAMP, ANGLE_CLAMP);
                        let dv = dv.clamp(-ANGLE_CLAMP, ANGLE_CLAMP);

                        // Apply combined small rotation about u then v (order is fine for small angles)
                        let (eye1, up1) = apply_center_rot(u, du, eye, up);
                        let (eye2, up2) = apply_center_rot(v, dv, eye1, up1);
                        eye = eye2;
                        up = up2;
                    } else {
                        // If projection failed, bail out of alignment
                        break;
                    }
                }
            }
        }

        // Commit results
        s.position.eye = eye;
        s.position.up = {
            // Re-orthonormalize (protect against drift)
            let view = s.position.target - s.position.eye;
            let view_n = if view.magnitude2() > 0.0 {
                view.normalize()
            } else {
                Vector3::new(0.0, 0.0, -1.0)
            };
            let right = view_n.cross(up);
            if right.magnitude2() > 0.0 {
                let right_n = right.normalize();
                right_n.cross(view_n).normalize()
            } else {
                up.normalize()
            }
        };
    }

    pub fn rotate(
        &self,
        dynamics_data: &CameraDynamicsData,
        from: ScreenPosition,
        to: ScreenPosition,
    ) {
        // Must know the locked world point from the pre-rotation pose.
        let p_lock = match from.world_position {
            Some(p) => p,
            None => return,
        };

        let mut s = self.state.write().expect("Dynamics write lock");

        // 1) Build unit direction to the locked point from the rotation center (globe center).
        //    If you orbit around world origin, target is (0,0,0). If not, replace with your center.
        let center = Point3::new(0.0, 0.0, 0.0);
        let vP = p_lock - center;
        if vP.magnitude2() == 0.0 {
            return;
        }
        let vP = vP.normalize();

        // 2) Get the current *pre-rotation* view ray for `to.xy`, in world space.
        //    You need a helper like:
        //      camera.view_ray_from_screen_with_pose(&s.position, to.xy) -> Option<Vector3<f64>>
        //    that returns a unit direction from the rotation center (globe center) for that pixel.
        let r_to = match view_ray_from_screen_with_pose(dynamics_data, &s.position, to.xy) {
            Some(d) => {
                if d.magnitude2() == 0.0 {
                    return;
                }
                d.normalize()
            }
            None => return,
        };

        // 3) Shortest-arc rotation that maps r_to -> vP. (Stable antiparallel handling)
        let q = shortest_arc_quat(r_to, vP);
        if q.is_none() {
            return;
        }
        let mut q = q.unwrap();
        // Sign-stabilize to kill flip/flop (q and -q are same rotation)
        if q.s < 0.0 {
            q = -q;
        }

        // 4) Apply to camera (about globe center): rotate eye and up, keep target at center.
        let eye_rel = s.position.eye - center;
        s.position.eye = center + q.rotate_vector(eye_rel);
        s.position.up = q.rotate_vector(s.position.up).normalize();
        s.position.target = center;

        // 5) Light re-orthonormalization of the basis to prevent micro drift.
        let view = s.position.target - s.position.eye;
        let view_n = if view.magnitude2() > 0.0 {
            view.normalize()
        } else {
            Vector3::new(0.0, 0.0, -1.0)
        };
        let right = view_n.cross(s.position.up).normalize();
        s.position.up = right.cross(view_n).normalize();

        s.rotation_pt = p_lock;

        // ---- helpers ----
        fn shortest_arc_quat(a: Vector3<f64>, b: Vector3<f64>) -> Option<Quaternion<f64>> {
            let va = if a.magnitude2() > 0.0 {
                a.normalize()
            } else {
                return None;
            };
            let vb = if b.magnitude2() > 0.0 {
                b.normalize()
            } else {
                return None;
            };

            let dot = va.dot(vb).clamp(-1.0, 1.0);
            // Aligned -> identity
            if 1.0 - dot < 1e-15 {
                return Some(Quaternion::one());
            }
            // Opposite -> 180° about any stable axis orthogonal to va
            if dot + 1.0 < 1e-15 {
                let mut axis = va.cross(Vector3::new(1.0, 0.0, 0.0));
                if axis.magnitude2() < 1e-18 {
                    axis = va.cross(Vector3::new(0.0, 1.0, 0.0));
                }
                if axis.magnitude2() < 1e-24 {
                    return None;
                }
                return Some(Quaternion::from_axis_angle(
                    axis.normalize(),
                    Rad(std::f64::consts::PI),
                ));
            }
            // General case
            let mut axis = va.cross(vb);
            let s = axis.magnitude();
            if s < 1e-18 {
                return Some(Quaternion::one());
            }
            axis /= s;
            let angle = s.atan2(dot); // stable atan2(|cross|, dot)
            Some(Quaternion::from_axis_angle(axis, Rad(angle)))
        }
    }

    /// Integrate momentum (if you still want inertial feel) & publish.
    pub fn update(&self, _dt: &core::time::Duration, camera: &Arc<Camera>) {
        let s = self.state.write().expect("Dynamics write lock");

        camera.update_dynamic_state(&s.position);
    }
}
