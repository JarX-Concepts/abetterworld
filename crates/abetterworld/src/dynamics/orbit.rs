use crate::{
    dynamics::Dynamics,
    world::{InputEvent, Key, MouseButton},
};

pub struct InputState {
    pub mouse_left_down: bool,
    pub mouse_position: (f32, f32),
    pub last_mouse_position: (f32, f32),
    // ...``
}

impl InputState {
    pub fn new() -> Self {
        InputState {
            mouse_left_down: false,
            mouse_position: (0.0, 0.0),
            last_mouse_position: (0.0, 0.0),
        }
    }

    pub fn reset(&mut self) {
        self.mouse_left_down = false;
        self.mouse_position = (0.0, 0.0);
    }

    pub fn process_input(&mut self, dynamics: &Dynamics, event: InputEvent) {
        match event {
            // Keyboard events
            InputEvent::KeyPressed(key) => {
                println!("Key pressed: {:?}", key);
                match key {
                    Key::ZoomIn => {
                        dynamics.zoom(1.0, true);
                    }
                    Key::ZoomOut => {
                        dynamics.zoom(1.0, false);
                    }

                    Key::ArrowUp => {
                        dynamics.tilt(1.0, true);
                    }
                    Key::ArrowDown => {
                        dynamics.tilt(1.0, false);
                    }

                    Key::ArrowLeft => {
                        dynamics.yaw(1.0, true);
                    }
                    Key::ArrowRight => {
                        dynamics.yaw(1.0, false);
                    }
                    _ => {}
                }
            }

            InputEvent::KeyReleased(key) => {
                println!("Key released: {:?}", key);
            }

            InputEvent::MouseMoved(x, y) => {
                let (x, y) = (x as f32, y as f32);
                self.mouse_position = (x, y);

                if self.mouse_left_down {
                    let (last_x, last_y) = self.last_mouse_position;
                    let delta_x = (x - last_x) as f64;
                    let delta_y = (y - last_y) as f64;

                    dynamics.yaw(delta_x.abs(), delta_x < 0.0);
                    dynamics.tilt(delta_y.abs(), delta_y < 0.0);

                    // Update last mouse position
                    self.last_mouse_position = (x, y);
                }
            }

            InputEvent::MouseScrolled(delta) => {
                dynamics.zoom(delta.abs() as f64, delta > 0.0);
            }

            InputEvent::MouseButtonPressed(button) => {
                if button == MouseButton::Left {
                    self.mouse_left_down = true;
                    self.last_mouse_position = self.mouse_position;
                }
            }

            InputEvent::MouseButtonReleased(button) => {
                if button == MouseButton::Left {
                    self.mouse_left_down = false;
                }
            }

            // --- gestures ---
            InputEvent::GesturePinch {
                begin,
                scale,
                velocity,
            } => {
                dynamics.gesture_pinch(begin, scale, velocity);
            }

            InputEvent::GestureOrbit {
                begin,
                dx,
                dy,
                vx,
                vy,
            } => {
                dynamics.gesture_orbit(begin, dx, dy, vx, vy);
            }

            InputEvent::GestureTranslate {
                begin,
                dx,
                dy,
                vx,
                vy,
            } => {
                dynamics.gesture_translate(begin, dx, dy, vx, vy);
            }

            InputEvent::GestureRotate {
                begin,
                radians,
                velocity,
            } => {
                dynamics.gesture_rotate(begin, radians, velocity);
            }

            InputEvent::GestureDoubleTap { x, y } => {
                dynamics.gesture_double_tap(x, y);
            }

            InputEvent::GestureTouchDown { active, x, y } => {
                dynamics.gesture_touch_down(active, x, y);
            }

            // keep your default/unhandled case:
            default => {
                println!("Unhandled input event: {:?}", default);
            }
        }
    }
}
