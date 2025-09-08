use cgmath::{Point2, Point3};

use crate::{
    dynamics::{self, screen_to_world_on_ellipsoid, CameraDynamicsData, Dynamics, Ellipsoid},
    world::{InputEvent, MouseButton},
};

#[derive(Debug, Clone, Copy)]
pub struct ScreenPosition {
    pub x: f64,
    pub y: f64,
    pub world_position: Option<Point3<f64>>,
}

impl ScreenPosition {
    pub fn default() -> Self {
        ScreenPosition {
            x: 0.0,
            y: 0.0,
            world_position: None,
        }
    }
    pub fn new(x: f64, y: f64, dynamics_data: &CameraDynamicsData) -> Self {
        let world_position = screen_to_world_on_ellipsoid(
            Point2::new(x, y),
            dynamics_data,
            Ellipsoid::default(),
            0.0,
        );
        ScreenPosition {
            x,
            y,
            world_position,
        }
    }
}

pub struct InputState {
    pub mouse_button_states: [bool; MouseButton::Count as usize],
    pub mouse_position: ScreenPosition,
    pub position_on_press: [ScreenPosition; MouseButton::Count as usize],
    pub mouse_wheel_delta: f64,
}

impl InputState {
    pub fn new() -> Self {
        InputState {
            mouse_button_states: [false; MouseButton::Count as usize],
            position_on_press: [ScreenPosition::default(); MouseButton::Count as usize],
            mouse_position: ScreenPosition::default(),
            mouse_wheel_delta: 0.0,
        }
    }

    pub fn reset(&mut self) {
        *self = InputState::new();
    }

    pub fn queue_event(&mut self, dynamics_data: &CameraDynamicsData, event: InputEvent) {
        match event {
            InputEvent::MouseMoved(x, y) => {
                self.mouse_position = ScreenPosition::new(x as f64, y as f64, dynamics_data);
            }

            InputEvent::MouseScrolled(delta) => {
                self.mouse_wheel_delta += delta;
            }

            InputEvent::MouseButtonPressed(button) => {
                self.mouse_button_states[button as usize] = true;
                self.position_on_press[button as usize] = self.mouse_position;
            }

            InputEvent::MouseButtonReleased(button) => {
                self.mouse_button_states[button as usize] = false;
            }

            // keep your default/unhandled case:
            default => {
                println!("Unhandled input event: {:?}", default);
            }
        }
    }

    pub fn flush(&mut self, dynamics: &mut Dynamics) {
        dynamics.zoom_to(self.mouse_wheel_delta, self.mouse_position.world_position);

        self.mouse_wheel_delta = 0.0;
    }
}
