#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cgmath::{InnerSpace, Point2, Point3};
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    use crate::dynamics::world_to_screen_proj;
    use crate::{
        dynamics::{init_camera, screen_to_world_on_ellipsoid, Dynamics, Ellipsoid, InputState},
        world::MouseButton,
        InputEvent,
    };

    #[test]
    fn test_dynamics() {
        // Start near NYC, some altitude so we're not inside the ellipsoid
        let nyc_pt = Point3::new(40.7128, -74.0060, 100_000.0);

        // Camera
        let camera = Arc::new(init_camera(nyc_pt));
        camera.set_viewport(1024.0, 768.0);
        camera.update();

        // Dynamics/model + input
        let mut model = Dynamics::new(camera.position());
        let mut im = InputState::new();

        // Start at the center of the viewport
        let mut mouse = Point2::new(512.0_f64, 384.0_f64);

        // Move mouse there & press to begin a drag
        im.queue_event(
            &camera.dynamics(),
            &mut model,
            InputEvent::MouseMoved(mouse.x, mouse.y),
            &camera,
        );
        im.queue_event(
            &camera.dynamics(),
            &mut model,
            InputEvent::MouseButtonPressed(MouseButton::Left),
            &camera,
        );
        im.flush(&mut model);
        camera.update();

        let dynamics = camera.dynamics();
        eprintln!("proj_inv: {:?}", dynamics.proj_inv);
        eprintln!("view_inv: {:?}", dynamics.view_inv);

        // Baseline world position under the cursor (ellipsoid, elevation 0)
        let ellipsoid = Ellipsoid::default();
        let baseline = screen_to_world_on_ellipsoid(
            Point2::new(mouse.x, mouse.y),
            &camera.dynamics(),
            ellipsoid,
            0.0,
        )
        .expect("baseline world pos");

        eprintln!("point: {:?}", baseline);

        let check_pos = world_to_screen_proj(baseline, dynamics.viewport_wh, &dynamics.proj_view)
            .expect("world to screen proj");

        let drift = (mouse - check_pos).magnitude();
        assert!(
            drift < 0.5,
            "Positions Do Not Match: {:?}, {:?} ",
            mouse,
            check_pos,
        );

        // Deterministic RNG (so failures reproduce)
        let mut rng = StdRng::seed_from_u64(42);

        let tol_m = 1.0; // 1 m tolerance

        // Jitter the mouse ~20 frames, 0..5 px each axis (random sign), and verify lock
        for i in 0..20 {
            // Random 0..=5, random sign
            let dx = (rng.gen_range(0.0..=5.0)) * if rng.gen_bool(0.5) { 1.0 } else { -1.0 };
            let dy = (rng.gen_range(0.0..=5.0)) * if rng.gen_bool(0.5) { 1.0 } else { -1.0 };
            mouse.x += dx;
            mouse.y += dy;

            // Feed input + integrate one frame (no momentum assumed)
            im.queue_event(
                &camera.dynamics(),
                &mut model,
                InputEvent::MouseMoved(mouse.x, mouse.y),
                &camera,
            );
            im.flush(&mut model);
            model.update(&core::time::Duration::from_millis(16), &camera);
            camera.update();

            // World position under the *new* cursor location after the rotation
            let world_pos = screen_to_world_on_ellipsoid(
                Point2::new(mouse.x, mouse.y),
                &camera.dynamics(),
                ellipsoid,
                0.0,
            )
            .expect("world pos");

            let drift = (world_pos - baseline).magnitude();

            // Helpful debug print on failure thresholds
            if drift > tol_m {
                eprintln!(
                    "Frame {}: drift {:.9} m exceeds tol {:.9} m; mouse=({:.1},{:.1}) d=({:+.1},{:+.1}) world={:?} baseline={:?}",
                    i, drift, tol_m, mouse.x, mouse.y, dx, dy, world_pos, baseline
                );
            }

            assert!(
                drift <= tol_m,
                "Cursor lock drift too large: {:.9} m (tol {:.9} m) at frame {}",
                drift,
                tol_m,
                i
            );
        }

        // Release mouse (optional; ensures clean input state for other tests)
        im.queue_event(
            &camera.dynamics(),
            &mut model,
            InputEvent::MouseButtonReleased(MouseButton::Left),
            &camera,
        );
        im.flush(&mut model);
    }
}
