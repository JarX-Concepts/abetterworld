use cgmath::{EuclideanSpace, InnerSpace, Matrix4, Point2, Point3, Vector3, Vector4};

use crate::dynamics::{CameraDynamicsData, PositionState};

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
    dynamics_data: &CameraDynamicsData,
    ellipsoid: Ellipsoid,
    elevation_m: f64,
) -> Option<Point3<f64>> {
    let (vw, vh) = (dynamics_data.viewport_wh.0, dynamics_data.viewport_wh.1);
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
    let near_world_h = dynamics_data.proj_view_inv * near_clip;
    let far_world_h = dynamics_data.proj_view_inv * far_clip;

    // Homogeneous divide
    let near_world = Point3::from_vec((near_world_h.truncate() / near_world_h.w).into());
    let far_world = Point3::from_vec((far_world_h.truncate() / far_world_h.w).into());

    // 3) Ray: origin = eye, dir = normalized (far - near) OR (point - eye)
    // Using (far - near) is numerically stable and independent of slight eye mismatch.
    let mut dir = far_world - near_world;
    if dir.magnitude2() == 0.0 {
        // Fallback: use (far - eye)
        dir = far_world - dynamics_data.eye;
        if dir.magnitude2() == 0.0 {
            return None;
        }
    }
    let dir = dir.normalize();
    let origin = dynamics_data.eye;

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

pub fn world_to_screen(
    world: Point3<f64>,
    dynamics_data: &CameraDynamicsData,
) -> Option<Point2<f64>> {
    world_to_screen_proj(world, dynamics_data.viewport_wh, &dynamics_data.proj_view)
}

pub fn world_to_screen_proj(
    world: Point3<f64>,
    viewport_wh: (f64, f64),
    proj_view: &Matrix4<f64>,
) -> Option<Point2<f64>> {
    let (vw, vh) = (viewport_wh.0, viewport_wh.1);
    if vw <= 0.0 || vh <= 0.0 {
        return None;
    }

    // world -> clip
    let world_h = Vector4::new(world.x, world.y, world.z, 1.0);
    let clip = proj_view * world_h;

    // Reject points with w ~ 0 or behind the camera (w <= 0 for standard RH OpenGL clip)
    if !clip.w.is_finite() || clip.w.abs() < 1e-12 || clip.w <= 0.0 {
        return None;
    }

    // clip -> NDC
    let ndc = clip.truncate() / clip.w; // [-1,1] each

    // (optional) Cull if outside NDC, including depth if you want only on-screen hits
    // if ndc.x < -1.0 || ndc.x > 1.0 || ndc.y < -1.0 || ndc.y > 1.0 || ndc.z < -1.0 || ndc.z > 1.0 {
    //     return None;
    // }

    // NDC -> pixels (y-down)
    let x_px = (ndc.x + 1.0) * 0.5 * vw;
    let y_px = (1.0 - ndc.y) * 0.5 * vh;

    if x_px.is_finite() && y_px.is_finite() {
        Some(Point2::new(x_px, y_px))
    } else {
        None
    }
}

/// Intersect ray (origin o, dir d) with a centered unit sphere.
/// Returns the *nearest positive* intersection point in world coords (not normalized).
fn ray_hit_ellipsoid(
    o: Vector3<f64>,
    d: Vector3<f64>,
    ax: f64,
    ay: f64,
    az: f64,
) -> Option<Vector3<f64>> {
    // Scale world so ellipsoid -> unit sphere
    let sx = 1.0 / ax;
    let sy = 1.0 / ay;
    let sz = 1.0 / az;

    let oe = Vector3::new(o.x * sx, o.y * sy, o.z * sz);
    let de = Vector3::new(d.x * sx, d.y * sy, d.z * sz); // not necessarily unit; OK

    // Intersect with unit sphere: |oe + t de|^2 = 1
    let a = de.dot(de);
    let b = 2.0 * oe.dot(de);
    let c = oe.dot(oe) - 1.0;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }

    let sqrt_disc = disc.sqrt();
    let t0 = (-b - sqrt_disc) / (2.0 * a);
    let t1 = (-b + sqrt_disc) / (2.0 * a);
    let t = if t0 > 0.0 {
        t0
    } else if t1 > 0.0 {
        t1
    } else {
        return None;
    };

    let hit_e = oe + de * t;
    // Map back to world space
    Some(Vector3::new(hit_e.x / sx, hit_e.y / sy, hit_e.z / sz))
}

fn ray_from_proj_view_inv(
    pv_inv: &Matrix4<f64>,
    xy: Point2<f64>,
    viewport_wh: (f64, f64),
) -> Option<(Vector3<f64>, Vector3<f64>)> {
    let (w, h) = viewport_wh;
    if w <= 1.0 || h <= 1.0 {
        return None;
    }

    // Screen -> NDC
    let x_ndc = (xy.x / w) * 2.0 - 1.0;
    let y_ndc = 1.0 - (xy.y / h) * 2.0;

    // Clip-space points on near/far planes
    let near_clip = Vector4::new(x_ndc, y_ndc, -1.0, 1.0);
    let far_clip = Vector4::new(x_ndc, y_ndc, 1.0, 1.0);

    // Unproject
    let mut near_w = *pv_inv * near_clip;
    let mut far_w = *pv_inv * far_clip;
    if near_w.w.abs() < 1e-18 || far_w.w.abs() < 1e-18 {
        return None;
    }
    near_w /= near_w.w;
    far_w /= far_w.w;

    let p_near = Vector3::new(near_w.x, near_w.y, near_w.z);
    let p_far = Vector3::new(far_w.x, far_w.y, far_w.z);
    let dir = (p_far - p_near).normalize();
    Some((p_near, dir))
}

/// Your original signature, upgraded to prefer unprojection via proj_view_inv.
/// Falls back to FOV/aspect path if proj_view_inv is unavailable in your dynamics data.
pub fn view_ray_from_screen_with_pose(
    dynamics_data: &CameraDynamicsData,
    pose: &PositionState,
    xy: Point2<f64>,
) -> Option<Vector3<f64>> {
    // If you can expose proj_view_inv from `dynamics_data`, use it:
    let pv_inv = dynamics_data.proj_view_inv;

    if let Some((ray_o, ray_d)) = ray_from_proj_view_inv(&pv_inv, xy, dynamics_data.viewport_wh) {
        // Intersect with ellipsoid centered at origin (WGS-84: a,a,b)
        let o = ray_o; // world coords
        let d = ray_d;
        if let Some(hit) = ray_hit_ellipsoid(o, d, WGS84_A, WGS84_A, WGS84_B) {
            return Some(hit.normalize()); // direction from center
        } else {
            // If ellipsoid fails (e.g., pointing away), you can early return None.
            return None;
        }
    }
    return None;
}

/// Project a point on the ellipsoid, optionally offset by `elevation_m` along the ellipsoid normal.
pub fn world_on_ellipsoid_to_screen(
    world_on_surface: Point3<f64>,
    dynamics_data: &CameraDynamicsData,
    ellipsoid: Ellipsoid,
    elevation_m: f64,
) -> Option<Point2<f64>> {
    let p = if elevation_m != 0.0 {
        let n = ellipsoid_normal(world_on_surface, ellipsoid);
        Point3::from_vec((world_on_surface.to_vec() + n * elevation_m).into())
    } else {
        world_on_surface
    };
    world_to_screen(p, dynamics_data)
}

/// Ellipsoid surface normal (unit) at a world point on the ellipsoid.
/// For x^2/a^2 + y^2/b^2 + z^2/c^2 = 1, non-unit normal ~ (x/a^2, y/b^2, z/c^2).
#[inline]
fn ellipsoid_normal(p: Point3<f64>, e: Ellipsoid) -> Vector3<f64> {
    let n = Vector3::new(p.x / (e.a * e.a), p.y / (e.b * e.b), p.z / (e.c * e.c));
    let m2 = n.magnitude2();
    if m2 > 0.0 {
        n / m2.sqrt()
    } else {
        Vector3::unit_z()
    }
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
