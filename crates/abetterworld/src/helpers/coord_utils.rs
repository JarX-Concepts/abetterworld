use cgmath::{Matrix3, Point3, Rad, Vector3};

/// Converts geodetic coordinates (latitude, longitude, elevation) to Y-up ECEF.
/// Assumes WGS84 ellipsoid.
pub fn geodetic_to_ecef_y_up(lat_deg: f64, lon_deg: f64, elevation_m: f64) -> (f64, f64, f64) {
    // WGS84 constants
    const A: f64 = 6378137.0; // semi-major axis in meters
    const E2: f64 = 6.69437999014e-3; // first eccentricity squared

    // Convert degrees to radians
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();

    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let sin_lon = lon.sin();
    let cos_lon = lon.cos();

    // Prime vertical radius of curvature
    let n = A / (1.0 - E2 * sin_lat * sin_lat).sqrt();

    // Standard ECEF (Z-up)
    let x_std = (n + elevation_m) * cos_lat * cos_lon;
    let y_std = (n + elevation_m) * cos_lat * sin_lon;
    let z_std = (n * (1.0 - E2) + elevation_m) * sin_lat;

    // Convert to Y-up:
    // x = x
    // y = z (up)
    // z = y
    let x_yup = x_std;
    let y_yup = z_std;
    let z_yup = y_std;

    (x_yup, y_yup, z_yup)
}

/// Converts geodetic coordinates (lat, lon, elevation) to standard Z-up ECEF coordinates.
/// Assumes WGS84 ellipsoid.
/// - `lat_deg` and `lon_deg` are in degrees
/// - `elevation_m` is in meters above sea level
pub fn geodetic_to_ecef_z_up(lat_deg: f64, lon_deg: f64, elevation_m: f64) -> Point3<f64> {
    // WGS84 constants
    const A: f64 = 6378137.0; // semi-major axis in meters
    const E2: f64 = 6.69437999014e-3; // first eccentricity squared

    // Convert degrees to radians
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();

    let sin_lat = lat.sin();
    let cos_lat = lat.cos();
    let sin_lon = lon.sin();
    let cos_lon = lon.cos();

    // Prime vertical radius of curvature
    let n = A / (1.0 - E2 * sin_lat * sin_lat).sqrt();

    // Standard ECEF Z-up
    let x = (n + elevation_m) * cos_lat * cos_lon;
    let y = (n + elevation_m) * cos_lat * sin_lon;
    let z = (n * (1.0 - E2) + elevation_m) * sin_lat;

    Point3::new(x, y, z)
}

pub fn hpr_to_forward_up(heading: f64, pitch: f64, roll: f64) -> (Vector3<f64>, Vector3<f64>) {
    let (h, p, r) = (
        Rad(heading.to_radians()),
        Rad(pitch.to_radians()),
        Rad(roll.to_radians()),
    );

    // Rotation matrices
    let rh = Matrix3::from_angle_y(h); // heading (yaw)
    let rp = Matrix3::from_angle_x(p); // pitch
    let rr = Matrix3::from_angle_z(r); // roll

    // Combined rotation: heading → pitch → roll
    let rot = rh * rp * rr;

    // Apply to basis vectors
    let forward = rot * Vector3::new(0.0, 0.0, -1.0);
    let up = rot * Vector3::new(0.0, 1.0, 0.0);

    (forward, up)
}

pub fn target_from_distance(eye: Point3<f64>, forward: &Vector3<f64>, dist: f64) -> Point3<f64> {
    eye + forward * dist
}
