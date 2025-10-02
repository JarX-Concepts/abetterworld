use cgmath::{EuclideanSpace, InnerSpace, Matrix4, Point2, Point3, Rad, Vector3, Vector4};

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

pub fn proj_reverse_z_infinite_f64(fovy: Rad<f64>, aspect: f64, near: f64) -> Matrix4<f64> {
    let f = 1.0 / (0.5 * fovy.0).tan();
    // Columns shown (col-major):
    // col0 = [ f/aspect, 0, 0, 0 ]
    // col1 = [ 0, f, 0, 0 ]
    // col2 = [ 0, 0, 0, -1 ]   // w' = -z_view (positive for z_view<0)
    // col3 = [ 0, 0,  near, 0 ]// z' = near * 1  (reverse-Z, infinite far)
    Matrix4::new(
        f / aspect,
        0.0,
        0.0,
        0.0, // col0
        0.0,
        f,
        0.0,
        0.0, // col1
        0.0,
        0.0,
        0.0,
        -1.0, // col2   (col2.w = -1)
        0.0,
        0.0,
        near,
        0.0, // col3   (col3.z = near)
    )
}

pub fn proj_reverse_z_infinite_inv_f64(fovy: Rad<f64>, aspect: f64, near: f64) -> Matrix4<f64> {
    let f = 1.0 / (0.5 * fovy.0).tan();
    // Inverse columns:
    // col0 = [ aspect/f, 0, 0, 0 ]
    // col1 = [ 0, 1/f, 0, 0 ]
    // col2 = [ 0, 0, 0, 1/near ]
    // col3 = [ 0, 0, -1, 0 ]      // note the -1 here (1/s, with s = -1)
    Matrix4::new(
        aspect / f,
        0.0,
        0.0,
        0.0, // col0
        0.0,
        1.0 / f,
        0.0,
        0.0, // col1
        0.0,
        0.0,
        0.0,
        1.0 / near, // col2
        0.0,
        0.0,
        -1.0,
        0.0, // col3
    )
}

pub fn screen_to_world_on_ellipsoid(
    screen_px: Point2<f64>,
    dynamics: &CameraDynamicsData, // exposes proj_inv, view_inv, eye, viewport_wh
    ellipsoid: Ellipsoid,
    elevation_m: f64,
) -> Option<Point3<f64>> {
    let (vw, vh) = dynamics.viewport_wh;
    if vw <= 0.0 || vh <= 0.0 {
        eprintln!("Invalid viewport: vw={}, vh={}", vw, vh);
        return None;
    }

    // 1) Screen → NDC
    let x_ndc = (screen_px.x / vw) * 2.0 - 1.0;
    let y_ndc = 1.0 - (screen_px.y / vh) * 2.0;

    // 2) Build VIEW ray exactly like ray_from_proj_view
    let tan_half_fov_x = 1.0 / dynamics.proj.x.x; // m00 = f/aspect
    let tan_half_fov_y = 1.0 / dynamics.proj.y.y; // m11 = f
    let dir_view = Vector3::new(
        x_ndc * tan_half_fov_x,
        y_ndc * tan_half_fov_y,
        -1.0, // RH, forward = -Z
    )
    .normalize();

    // 3) Rotate to WORLD (w=0 to ignore translation)
    let dir_world4 = dynamics.view_inv * Vector4::new(dir_view.x, dir_view.y, dir_view.z, 0.0);
    let mut dir = Vector3::new(dir_world4.x, dir_world4.y, dir_world4.z).normalize();

    let origin = dynamics.eye;

    // Safety: if ray points away from Earth center, flip it
    if dir.dot(origin.to_vec()) > 0.0 {
        dir = -dir;
    }

    // 4) Intersect and elevate (unchanged)
    let t = intersect_ray_ellipsoid(origin, dir, ellipsoid)?;
    let p = origin + dir * t;
    Some(apply_elevation(p, ellipsoid, elevation_m))
}

// ----- small helpers -----

#[inline]
fn is_inside_ellipsoid(p: Point3<f64>, e: Ellipsoid) -> bool {
    let x = p.x / e.a;
    let y = p.y / e.b;
    let z = p.z / e.c;
    (x * x + y * y + z * z) <= 1.0
}

#[inline]
fn ellipsoid_normal(p: Point3<f64>, e: Ellipsoid) -> Vector3<f64> {
    let n_raw = Vector3::new(p.x / (e.a * e.a), p.y / (e.b * e.b), p.z / (e.c * e.c));
    if n_raw.magnitude2() > 0.0 {
        n_raw.normalize()
    } else {
        Vector3::unit_z()
    }
}

#[inline]
fn apply_elevation(p: Point3<f64>, e: Ellipsoid, elevation_m: f64) -> Point3<f64> {
    if elevation_m == 0.0 {
        return p;
    }
    let n = ellipsoid_normal(p, e); // outward
    Point3::from_vec((p.to_vec() + n * elevation_m).into())
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
        eprintln!("Invalid viewport: ({}, {})", vw, vh);
        return None;
    }

    let world_h = Vector4::new(world.x, world.y, world.z, 1.0);
    let clip = *proj_view * world_h;
    eprintln!("world={:?}, world_h={:?}, clip={:?}", world, world_h, clip);

    // Reject points with invalid w (but allow either sign)
    if !clip.w.is_finite() || clip.w.abs() < 1e-12 {
        return None;
    }

    // clip -> NDC
    let ndc = clip.truncate() / clip.w; // x,y∈[-1,1], z∈[0,1] for reverse-Z

    // Optional strict on-screen check
    // if ndc.x < -1.0 || ndc.x > 1.0 || ndc.y < -1.0 || ndc.y > 1.0 || ndc.z < 0.0 || ndc.z > 1.0 {
    //     eprintln!("Rejected off-screen ndc={:?}", ndc);
    //     return None;
    // }

    let x_px = (ndc.x + 1.0) * 0.5 * vw;
    let y_px = (1.0 - ndc.y) * 0.5 * vh;
    eprintln!("pixel=({}, {})", x_px, y_px);

    if x_px.is_finite() && y_px.is_finite() {
        Some(Point2::new(x_px, y_px))
    } else {
        None
    }
}

fn ray_hit_ellipsoid(
    o: Vector3<f64>,
    d: Vector3<f64>, // need not be unit; t is in world units
    ax: f64,
    ay: f64,
    az: f64,
) -> Option<Vector3<f64>> {
    let inv2x = 1.0 / (ax * ax);
    let inv2y = 1.0 / (ay * ay);
    let inv2z = 1.0 / (az * az);

    eprintln!("Origin o={:?}, dir d={:?}", o, d);
    eprintln!("Axes = ({:.3}, {:.3}, {:.3})", ax, ay, az);
    eprintln!("inv2 = ({:.6}, {:.6}, {:.6})", inv2x, inv2y, inv2z);

    let a = d.x * d.x * inv2x + d.y * d.y * inv2y + d.z * d.z * inv2z;
    let b = 2.0 * (o.x * d.x * inv2x + o.y * d.y * inv2y + o.z * d.z * inv2z);
    let mut c = o.x * o.x * inv2x + o.y * o.y * inv2y + o.z * o.z * inv2z - 1.0;
    let inside = c < 0.0;

    eprintln!("Quadratic coeffs: a={:.12}, b={:.12}, c={:.12}", a, b, c);
    eprintln!("Inside ellipsoid? {}", inside);

    const EPS_A: f64 = 1e-18;
    if a.abs() < EPS_A {
        eprintln!("Degenerate direction (a≈0)");
        return if inside { Some(o) } else { None }.map(|_| o);
    }

    let mut disc = b * b - 4.0 * a * c;
    eprintln!("Discriminant raw={:.12}", disc);
    const EPS_DISC: f64 = 1e-12;
    if disc < 0.0 && disc > -EPS_DISC {
        eprintln!("Clamping near-zero discriminant to 0");
        disc = 0.0;
    }
    if disc < 0.0 {
        eprintln!("No real roots (disc < 0)");
        return None;
    }
    let sqrt_disc = disc.sqrt();
    eprintln!("sqrt_disc={:.12}", sqrt_disc);

    let q = -0.5 * (b + if b >= 0.0 { sqrt_disc } else { -sqrt_disc });
    let t0 = q / a;
    let t1 = if q != 0.0 { c / q } else { -b / (2.0 * a) };

    eprintln!("q={:.12}, t0={:.12}, t1={:.12}", q, t0, t1);

    let (t_min, t_max) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
    eprintln!("t_min={:.12}, t_max={:.12}", t_min, t_max);

    let t = if inside {
        if t_max >= 0.0 {
            eprintln!("Inside: picking exit t_max={:.12}", t_max);
            t_max
        } else {
            eprintln!("Inside: no forward exit");
            return None;
        }
    } else {
        if t_min >= 0.0 {
            eprintln!("Outside: picking nearest root t_min={:.12}", t_min);
            t_min
        } else if t_max >= 0.0 {
            eprintln!("Outside: t_min<0, picking t_max={:.12}", t_max);
            t_max
        } else {
            eprintln!("Outside: both roots negative, no hit");
            return None;
        }
    };

    let hit = o + d * t;
    eprintln!("Hit point = {:?}", hit);
    Some(hit)
}

pub fn ray_from_proj_view(
    proj: &Matrix4<f64>,
    view_inv: &Matrix4<f64>,
    eye: Point3<f64>,
    pixel: Point2<f64>,
    viewport_wh: (f64, f64),
) -> Option<(Vector3<f64>, Vector3<f64>)> {
    let (w, h) = viewport_wh;
    if w <= 1.0 || h <= 1.0 {
        eprintln!("Invalid viewport size: w={}, h={}", w, h);
        return None;
    }

    // Screen -> NDC (-1..1), y up
    let x_ndc = (pixel.x / w) * 2.0 - 1.0;
    let y_ndc = 1.0 - (pixel.y / h) * 2.0;
    eprintln!(
        "pixel={:?}, viewport=({:.3},{:.3}), NDC=({:.6},{:.6})",
        pixel, w, h, x_ndc, y_ndc
    );

    // Get tangents of half-FOVs from the projection
    let tan_half_fov_x = 1.0 / proj.x.x; // proj[0][0]
    let tan_half_fov_y = 1.0 / proj.y.y; // proj[1][1]
    eprintln!(
        "proj[0][0]={:.6}, proj[1][1]={:.6}, tan_half_fov=({:.6},{:.6})",
        proj.x.x, proj.y.y, tan_half_fov_x, tan_half_fov_y
    );

    // View-space ray direction (RH, camera forward = -Z)
    let dir_view = Vector3::new(x_ndc * tan_half_fov_x, y_ndc * tan_half_fov_y, -1.0).normalize();
    eprintln!(
        "dir_view(before normalize) = ({:.6},{:.6},{:.6})",
        x_ndc * tan_half_fov_x,
        y_ndc * tan_half_fov_y,
        -1.0
    );
    eprintln!("dir_view(normalized) = {:?}", dir_view);

    // Rotate to world (ignore translation via w=0)
    let dir_world4 = *view_inv * Vector4::new(dir_view.x, dir_view.y, dir_view.z, 0.0);
    let mut dir_world = Vector3::new(dir_world4.x, dir_world4.y, dir_world4.z);
    eprintln!("dir_world4 = {:?}", dir_world4);
    eprintln!("dir_world (pre-normalize) = {:?}", dir_world);

    if dir_world.magnitude2() == 0.0 || !dir_world.magnitude2().is_finite() {
        eprintln!("Invalid dir_world vector: {:?}", dir_world);
        return None;
    }
    dir_world = dir_world.normalize();
    eprintln!("dir_world (normalized) = {:?}", dir_world);

    let origin = Vector3::new(eye.x, eye.y, eye.z);
    eprintln!("eye = {:?}, origin = {:?}", eye, origin);

    Some((origin, dir_world))
}

pub fn view_ray_from_screen_with_pose(
    dynamics_data: &CameraDynamicsData,
    _pose: &PositionState, // not needed; avoid unused warning
    xy: Point2<f64>,
) -> Option<Vector3<f64>> {
    let (ray_o, ray_d) = ray_from_proj_view(
        &dynamics_data.proj,
        &dynamics_data.view_inv,
        dynamics_data.eye,
        xy,
        dynamics_data.viewport_wh,
    )?;

    eprintln!(
        "view_ray_from_screen_with_pose: xy={:?}, ray_o={:?}, ray_d={:?}",
        xy, ray_o, ray_d
    );

    let hit = ray_hit_ellipsoid(ray_o, ray_d, WGS84_A, WGS84_A, WGS84_B)?;
    eprintln!("view_ray_from_screen_with_pose: hit={:?}", hit);

    // Verify hit lies on ellipsoid
    let eq_val = (hit.x * hit.x) / (WGS84_A * WGS84_A)
        + (hit.y * hit.y) / (WGS84_A * WGS84_A)
        + (hit.z * hit.z) / (WGS84_B * WGS84_B);
    eprintln!("ellipsoid eq check ≈ {:.12}", eq_val);

    let len2 = hit.magnitude2();
    if len2 == 0.0 || !len2.is_finite() {
        eprintln!("Invalid hit vector len2={}", len2);
        return None;
    }
    let dir_center = hit / len2.sqrt();
    eprintln!(
        "view_ray_from_screen_with_pose: dir_center={:?}",
        dir_center
    );

    Some(dir_center)
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

/// Solve quadratic for ray/ellipsoid intersection.
/// Ray: r(t) = O + t*D, t >= 0
/// Ellipsoid: (x/a)^2 + (y/b)^2 + (z/c)^2 = 1
pub fn intersect_ray_ellipsoid(
    origin: Point3<f64>,
    dir: Vector3<f64>, // need not be unit; t is in world units
    e: Ellipsoid,
) -> Option<f64> {
    // Precompute inverse-squared axes (metric matrix diag)
    let inv2 = Vector3::new(1.0 / (e.a * e.a), 1.0 / (e.b * e.b), 1.0 / (e.c * e.c));

    let o = Vector3::new(origin.x, origin.y, origin.z);
    let d = dir;

    // Quadratic: (o + t d)^T M (o + t d) = 1
    // A t^2 + B t + C = 0
    let a = d.x * d.x * inv2.x + d.y * d.y * inv2.y + d.z * d.z * inv2.z;
    let b = 2.0 * (o.x * d.x * inv2.x + o.y * d.y * inv2.y + o.z * d.z * inv2.z);
    let mut c = (o.x * o.x) * inv2.x + (o.y * o.y) * inv2.y + (o.z * o.z) * inv2.z - 1.0;

    // Inside? We'll want the exit root (positive t)
    let inside = c < 0.0;

    // Degenerate direction
    const EPS_A: f64 = 1e-18;
    if a.abs() < EPS_A {
        // Direction parallel to a tangent plane (or zero)
        return if inside { Some(0.0) } else { None };
    }

    // Discriminant with tolerance
    let mut disc = b * b - 4.0 * a * c;
    const EPS_DISC: f64 = 1e-12;
    if disc < 0.0 && disc > -EPS_DISC {
        disc = 0.0; // treat as tangent
    }
    if disc < 0.0 {
        return None;
    }

    let sqrt_disc = disc.sqrt();

    // Stable quadratic solution
    // q = -0.5 * (b + sign(b)*sqrt_disc)
    let q = -0.5 * (b + if b >= 0.0 { sqrt_disc } else { -sqrt_disc });
    let t0 = q / a;
    let t1 = if q != 0.0 { c / q } else { -b / (2.0 * a) }; // fallback if q==0

    // Order roots
    let (t_min, t_max) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };

    if inside {
        // expect t_min < 0 < t_max; take forward exit
        return if t_max >= 0.0 { Some(t_max) } else { None };
    }

    // Outside: smallest non-negative root
    if t_min >= 0.0 {
        Some(t_min)
    } else if t_max >= 0.0 {
        Some(t_max)
    } else {
        None
    }
}

pub fn view_ray_dir_from_screen(
    dyns: &CameraDynamicsData, // must expose proj_inv, view_inv, viewport_wh
    _pose: &PositionState,     // not used here, but handy if you vary per pose
    xy: Point2<f64>,
) -> Option<Vector3<f64>> {
    let (w, h) = dyns.viewport_wh;
    if w <= 1.0 || h <= 1.0 {
        return None;
    }

    // Screen -> NDC
    let x_ndc = (xy.x / w) * 2.0 - 1.0;
    let y_ndc = 1.0 - (xy.y / h) * 2.0;

    // Reverse-Z near plane point in VIEW space (z=1)
    let p_view_h = dyns.proj_inv * Vector4::new(x_ndc, y_ndc, 1.0, 1.0);
    if !p_view_h.w.is_finite() || p_view_h.w.abs() < 1e-18 {
        return None;
    }
    let p_view = p_view_h.truncate() / p_view_h.w;

    // View-space forward ray through that pixel. Negate if your convention is -Z forward.
    let dir_view = -(p_view.normalize());

    // Rotate to world; w=0 so translation is ignored
    let dir_world4 = dyns.view_inv * Vector4::new(dir_view.x, dir_view.y, dir_view.z, 0.0);
    let mut dir_world = Vector3::new(dir_world4.x, dir_world4.y, dir_world4.z);
    if dir_world.magnitude2() == 0.0 || !dir_world.magnitude2().is_finite() {
        return None;
    }
    dir_world = dir_world.normalize();

    Some(dir_world)
}
