use cgmath::{EuclideanSpace, Point2, Point3};

use crate::{
    dynamics::{
        self, screen_to_world_on_ellipsoid, world_to_screen, CameraDynamicsData, Dynamics,
        Ellipsoid,
    },
    world::{InputEvent, MouseButton},
};

#[derive(Debug, Clone, Copy)]
pub struct ScreenPosition {
    pub xy: Point2<f64>,
    pub world_position: Option<Point3<f64>>,
}

impl ScreenPosition {
    pub fn default() -> Self {
        ScreenPosition {
            xy: Point2::new(0.0, 0.0),
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
            xy: Point2::new(x, y),
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

    pub fn queue_event(
        &mut self,
        dynamics_data: &CameraDynamicsData,
        dynamics: &mut Dynamics,
        event: InputEvent,
    ) {
        match event {
            InputEvent::MouseMoved(x, y) => {
                self.mouse_position = ScreenPosition::new(x as f64, y as f64, dynamics_data);
                if self.mouse_button_states[MouseButton::Left as usize] {
                    dynamics.rotate(
                        dynamics_data,
                        self.position_on_press[MouseButton::Left as usize],
                        self.mouse_position,
                    );
                }

                /*                 let back_to_screen = world_to_screen(
                    self.mouse_position
                        .world_position
                        .unwrap_or(Point3::new(0.0, 0.0, 0.0)),
                    dynamics_data,
                );
                println!(
                    "screen pos: {:?} to world: {:?}",
                    self.mouse_position.xy, self.mouse_position.world_position
                );
                println!(
                    "world pos: {:?} back to screen: {:?}",
                    self.mouse_position.world_position, back_to_screen
                ); */
            }

            InputEvent::MouseScrolled(delta) => {
                self.mouse_wheel_delta += delta;

                dynamics.zoom(dynamics_data, delta, Some(self.mouse_position));
            }

            InputEvent::MouseButtonPressed(button) => {
                self.mouse_button_states[button as usize] = true;
                self.position_on_press[button as usize] = self.mouse_position;

                //dynamics.begin_gesture(self.mouse_position);
            }

            InputEvent::MouseButtonReleased(button) => {
                self.mouse_button_states[button as usize] = false;

                //dynamics.end_gesture();
            }

            // keep your default/unhandled case:
            default => {
                println!("Unhandled input event: {:?}", default);
            }
        }
    }

    pub fn flush(&mut self, dynamics: &mut Dynamics) {
        /*         if (self.mouse_wheel_delta != 0.0) {
            dynamics.zoom(self.mouse_wheel_delta, self.mouse_position.world_position);
        }

        if self.mouse_button_states[MouseButton::Left as usize] {
            let delta =
                self.mouse_position.xy - self.position_on_press[MouseButton::Left as usize].xy;
            dynamics.pull(
                Point2::from_vec(delta),
                self.position_on_press[MouseButton::Left as usize].world_position,
            );
            self.position_on_press[MouseButton::Left as usize] = self.mouse_position;
        }

        self.mouse_wheel_delta = 0.0; */
    }
}
