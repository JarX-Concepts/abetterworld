use cgmath::{EuclideanSpace, InnerSpace, Matrix4, Point2, Point3, Vector3};

use crate::dynamics::PositionState;

/// WGS84 radii (meters)
pub const WGS84_A: f64 = 6_378_137.0; // equatorial (X,Y)
pub const WGS84_B: f64 = 6_356_752.314245; // polar (Z)

#[derive(Debug, Clone, Copy)]
pub struct Ellipsoid {
    pub a: f64, // X radius
    pub b: f64, // Y radius
    pub c: f64, // Z radius
}

impl Default for Ellipsoid {
    fn default() -> Self {
        Ellipsoid {
            a: WGS84_A,
            b: WGS84_A,
            c: WGS84_B,
        }
    }
}

/// Convert screen pixel -> ray in world space, then intersect with ellipsoid.
/// `screen_px`: (x,y) in pixels, origin at top-left of the viewport.
/// `elevation_m`: add this many meters along the ellipsoid normal (use 0.0 if not needed).
pub fn screen_to_world_on_ellipsoid(
    screen_px: Point2<f64>,
    inv_view_proj: &Matrix4<f64>,
    cam: &PositionState,
    ellipsoid: Ellipsoid,
    elevation_m: f64,
) -> Option<Point3<f64>> {
    let (vw, vh) = (cam.viewport_wh.0 as f64, cam.viewport_wh.1 as f64);
    if vw <= 0.0 || vh <= 0.0 {
        return None;
    }

    // 1) Screen -> NDC (x,y in [-1,1], y flipped)
    let x_ndc = (screen_px.x / vw) * 2.0 - 1.0;
    let y_ndc = 1.0 - (screen_px.y / vh) * 2.0;

    // Two clip-space points along the view ray (z = -1 near, z = 1 far in OpenGL-style clip space)
    let near_clip = cgmath::Vector4::new(x_ndc, y_ndc, -1.0, 1.0);
    let far_clip = cgmath::Vector4::new(x_ndc, y_ndc, 1.0, 1.0);

    // 2) Unproject to world space
    let near_world_h = inv_view_proj * near_clip;
    let far_world_h = inv_view_proj * far_clip;

    // Homogeneous divide
    let near_world = Point3::from_vec((near_world_h.truncate() / near_world_h.w).into());
    let far_world = Point3::from_vec((far_world_h.truncate() / far_world_h.w).into());

    // 3) Ray: origin = eye, dir = normalized (far - near) OR (point - eye)
    // Using (far - near) is numerically stable and independent of slight eye mismatch.
    let mut dir = far_world - near_world;
    if dir.magnitude2() == 0.0 {
        // Fallback: use (far - eye)
        dir = far_world - cam.eye;
        if dir.magnitude2() == 0.0 {
            return None;
        }
    }
    let dir = dir.normalize();
    let origin = cam.eye;

    // 4) Intersect ray with ellipsoid centered at origin (ECEF assumption)
    let t = intersect_ray_ellipsoid(origin, dir, ellipsoid)?;

    // 5) Point at intersection
    let p = origin + dir * t;

    if elevation_m == 0.0 {
        return Some(p);
    }

    // 6) Offset by elevation along ellipsoid surface normal
    // For ellipsoid x^2/a^2 + y^2/b^2 + z^2/c^2 = 1, the (non-unit) normal at p is:
    // n ~ (x/a^2, y/b^2, z/c^2).
    let n_raw = Vector3::new(
        p.x / (ellipsoid.a * ellipsoid.a),
        p.y / (ellipsoid.b * ellipsoid.b),
        p.z / (ellipsoid.c * ellipsoid.c),
    );
    let n = if n_raw.magnitude2() > 0.0 {
        n_raw.normalize()
    } else {
        Vector3::unit_z()
    };

    Some(Point3::from_vec((p.to_vec() + n * elevation_m).into()))
}

/// Solve quadratic for ray/ellipsoid intersection.
/// Ray: r(t) = O + t*D, t >= 0
/// Ellipsoid: (x/a)^2 + (y/b)^2 + (z/c)^2 = 1
fn intersect_ray_ellipsoid(origin: Point3<f64>, dir: Vector3<f64>, e: Ellipsoid) -> Option<f64> {
    // Scale space so ellipsoid becomes unit sphere:
    // Define O' = (Ox/a, Oy/b, Oz/c), D' = (Dx/a, Dy/b, Dz/c)
    let o = Vector3::new(origin.x / e.a, origin.y / e.b, origin.z / e.c);
    let d = Vector3::new(dir.x / e.a, dir.y / e.b, dir.z / e.c);

    // Intersection with unit sphere: ||O' + t D'||^2 = 1
    // => (d·d) t^2 + 2 (o·d) t + (o·o - 1) = 0
    let a = d.dot(d);
    let b = 2.0 * o.dot(d);
    let c = o.dot(o) - 1.0;

    // If origin is already inside the ellipsoid, treat t=0 as "hit" (or push outward)
    if c <= 0.0 {
        return Some(0.0);
    }

    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }

    let sqrt_disc = disc.sqrt();
    let t0 = (-b - sqrt_disc) / (2.0 * a);
    let t1 = (-b + sqrt_disc) / (2.0 * a);

    // We want the smallest non-negative t
    let t_min = if t0 >= 0.0 && t1 >= 0.0 {
        t0.min(t1)
    } else if t0 >= 0.0 {
        t0
    } else if t1 >= 0.0 {
        t1
    } else {
        // Both behind the camera
        return None;
    };

    Some(t_min)
}
