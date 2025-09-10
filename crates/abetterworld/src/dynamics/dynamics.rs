use crate::dynamics::{
    view_ray_from_screen_with_pose, world_to_screen, Camera, CameraDynamicsData, PositionState,
    ScreenPosition,
};
use cgmath::{
    EuclideanSpace, InnerSpace, One, Point2, Point3, Quaternion, Rad, Rotation, Rotation3, Vector3,
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
