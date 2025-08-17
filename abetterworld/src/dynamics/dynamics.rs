use crate::dynamics::{Camera, EARTH_RADIUS_M};
use cgmath::{
    Deg, EuclideanSpace, InnerSpace, Point3, Quaternion, Rotation, Rotation3, Vector2, Vector3,
};
use std::sync::{Arc, RwLock};

static ROTATION_SENSITIVITY: f64 = 0.000000005;

enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub struct PositionState {
    pub eye: Point3<f64>,
    pub target: Point3<f64>,
    pub up: Vector3<f64>,
}

#[derive(Debug, Clone)]
pub struct DynamicsState {
    pub position: PositionState,
    // add what we need
}

#[derive(Debug)]
pub struct Dynamics {
    state: RwLock<DynamicsState>,
    // add what we need for momentum features
}

impl Dynamics {
    pub fn new(position: PositionState) -> Self {
        Self {
            state: RwLock::new(DynamicsState { position }),
        }
    }

    pub fn height_above_terrain(&self) -> f64 {
        let cam_world = self.state.read().unwrap().position.eye.to_vec();
        // Distance from camera to Earth's center
        let distance_to_center = cam_world.magnitude();
        // Height above terrain is distance to center minus Earth's radius
        distance_to_center - EARTH_RADIUS_M
    }

    /// move the eye closer/further from the target along the view direction
    pub fn zoom(&self, amount: f64, in_flag: bool) {
        let height = self.height_above_terrain();
        let zoom_amount = amount * height * 0.1;

        let mut state = self.state.write().unwrap();
        let view_dir = (state.position.target - state.position.eye).normalize();

        if in_flag {
            state.position.eye += view_dir * zoom_amount;
        } else {
            state.position.eye -= view_dir * zoom_amount;
        }
    }

    /// rotate camera up/down around the camera-right axis
    pub fn tilt(&self, amount: f64, up_flag: bool) {
        let height = self.height_above_terrain();
        let pan_amount = amount * height * ROTATION_SENSITIVITY;

        let angle = if up_flag {
            Deg(-pan_amount as f64)
        } else {
            Deg(pan_amount as f64)
        };

        let mut state = self.state.write().unwrap();
        let view_vec = state.position.eye - state.position.target;
        let right = (state.position.target - state.position.eye)
            .normalize()
            .cross(state.position.up)
            .normalize();
        let q: Quaternion<f64> = Quaternion::from_axis_angle(right, angle);
        let new_view = q.rotate_vector(view_vec);
        state.position.eye = state.position.target + new_view;
        state.position.up = q.rotate_vector(state.position.up).normalize();
    }

    /// rotate camera left/right around the world-up axis
    pub fn yaw(&self, amount: f64, left_flag: bool) {
        let height = self.height_above_terrain();
        let yaw_amount = amount * height * ROTATION_SENSITIVITY;

        let angle = if left_flag {
            Deg(-yaw_amount as f64)
        } else {
            Deg(yaw_amount as f64)
        };

        let mut state = self.state.write().unwrap();
        let view_vec = state.position.eye - state.position.target;
        let q: Quaternion<f64> = Quaternion::from_axis_angle(state.position.up, angle);
        let new_view = q.rotate_vector(view_vec);
        state.position.eye = state.position.target + new_view;
    }

    pub fn update(&self, time: &core::time::Duration, camera: &Arc<Camera>) {
        camera.update_dynamic_state(&self.state.read().unwrap().position);
    }
}
