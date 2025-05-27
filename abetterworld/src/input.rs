use crate::{camera::Camera, InputEvent, Key};

pub fn process_input(camera: &mut Camera, event: InputEvent) {
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
                    let pan_amount = (0.0, -height * 0.05).into(); // Pan west
                    camera.pan(pan_amount);
                }
                Key::ArrowDown => {
                    let pan_amount = (0.0, height * 0.05).into(); // Pan east
                    camera.pan(pan_amount);
                }

                Key::ArrowLeft => {
                    let pan_amount = (-height * 0.05, 0.0).into(); // Pan west
                    camera.pan(pan_amount);
                }
                Key::ArrowRight => {
                    let pan_amount = (height * 0.05, 0.0).into(); // Pan east
                    camera.pan(pan_amount);
                }
                _ => {}
            }
        }

        InputEvent::KeyReleased(key) => {
            println!("Key released: {:?}", key);
        }

        // Mouse events
        InputEvent::MouseMoved { delta } => {
            println!("Mouse moved: {:?}", delta);
            // rotate camera or hover effect
        }

        InputEvent::MouseScrolled { delta } => {
            println!("Mouse scrolled: {}", delta);
            // zoom camera
        }

        InputEvent::MouseButtonPressed(button) => {
            println!("Mouse button pressed: {:?}", button);
        }

        InputEvent::MouseButtonReleased(button) => {
            println!("Mouse button released: {:?}", button);
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
