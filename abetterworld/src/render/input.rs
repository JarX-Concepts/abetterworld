use std::sync::Arc;

use cgmath::Deg;

use crate::{render::camera::Camera, InputEvent, Key, MouseButton};

static ROTATION_SENSITIVITY: f64 = 0.000000005;

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

    pub fn process_input(&mut self, camera: &Arc<Camera>, event: InputEvent) {
        let height = camera.height_above_terrain();

        match event {
            // Keyboard events
            InputEvent::KeyPressed(key) => {
                println!("Key pressed: {:?}", key);
                match key {
                    Key::ZoomIn => {
                        let zoom_amount = height * 0.1;
                        camera.zoom(zoom_amount);
                    }
                    Key::ZoomOut => {
                        let zoom_amount = -height * 0.1;
                        camera.zoom(zoom_amount);
                    }

                    Key::ArrowUp => {
                        let pan_amount = -height * ROTATION_SENSITIVITY;
                        camera.tilt(Deg(pan_amount as f64));
                    }
                    Key::ArrowDown => {
                        let pan_amount = height * ROTATION_SENSITIVITY;
                        camera.tilt(Deg(pan_amount as f64));
                    }

                    Key::ArrowLeft => {
                        let pan_amount = -height * ROTATION_SENSITIVITY;
                        camera.yaw(Deg(pan_amount as f64));
                    }
                    Key::ArrowRight => {
                        let pan_amount = height * ROTATION_SENSITIVITY;
                        camera.yaw(Deg(pan_amount as f64));
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

                    let height = camera.height_above_terrain();
                    let delta_yaw = -delta_x * ROTATION_SENSITIVITY * height;
                    let delta_pitch = -delta_y * ROTATION_SENSITIVITY * height;

                    camera.yaw(Deg(delta_yaw));
                    camera.tilt(Deg(delta_pitch));

                    // Update last mouse position
                    self.last_mouse_position = (x, y);
                }
            }

            InputEvent::MouseScrolled(delta) => {
                let zoom_amount = (delta as f64) * height * 0.001;
                camera.zoom(zoom_amount);
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

            // Touch events
            InputEvent::TouchStart { id, position } => {
                println!("Touch start: id={}, position={:?}", id, position);
                // start tracking touch gesture
            }

            InputEvent::TouchMove { id, position } => {
                println!("Touch move: id={}, position={:?}", id, position);
                // update gesture tracking
            }

            InputEvent::TouchEnd { id } => {
                println!("Touch end: id={}", id);
                // finalize gesture
            }
        }
    }
}
