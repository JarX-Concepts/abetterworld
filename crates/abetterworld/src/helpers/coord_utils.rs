use cgmath::{Matrix3, Point3, Rad, Vector3};

const A: f64 = 6378137.0; // semi-major axis (m)
const F: f64 = 1.0 / 298.257_223_563; // flattening
const B: f64 = A * (1.0 - F); // semi-minor axis
const E2: f64 = (A * A - B * B) / (A * A); // first eccentricity^2
const EP2: f64 = (A * A - B * B) / (B * B); // second eccentricity^2

/// Convert ECEF (meters) to geodetic (lat, lon in degrees, h_ellip in meters).
/// Uses Bowring’s closed-form with one refinement; robust for all latitudes.
pub fn ecef_to_lla_wgs84(ecef: Point3<f64>) -> (f64, f64, f64) {
    let x = ecef.x;
    let y = ecef.y;
    let z = ecef.z;

    let p = (x * x + y * y).sqrt();
    let lon = y.atan2(x); // [-π, π]

    // Bowring’s formula for initial latitude
    let theta = (z * A).atan2(p * B);
    let sin_theta = theta.sin();
    let cos_theta = theta.cos();

    // Geodetic latitude
    let lat = (z + EP2 * B * sin_theta.powi(3)).atan2(p - E2 * A * cos_theta.powi(3));

    // Prime vertical radius of curvature
    let sin_lat = lat.sin();
    let n = A / (1.0 - E2 * sin_lat * sin_lat).sqrt();

    // Ellipsoidal height (above WGS-84)
    let h = p / lat.cos() - n;

    (lat.to_degrees(), normalize_lon_deg(lon.to_degrees()), h)
}

fn normalize_lon_deg(mut lon: f64) -> f64 {
    // Map to (-180, 180]
    lon = ((lon + 180.0) % 360.0 + 360.0) % 360.0 - 180.0;
    lon
}

/// Orthometric (MSL) height = ellipsoidal height − geoid undulation.
/// If you have a geoid model (e.g., EGM2008), pass a function that returns N(lat, lon) in meters.
/// If not, this returns `None` and you can fall back to ellipsoidal height.
pub fn ellipsoidal_to_msl(
    lat_deg: f64,
    lon_deg: f64,
    h_ellip_m: f64,
    geoid_undulation_m: Option<fn(f64, f64) -> f64>,
) -> Option<f64> {
    if let Some(geoid_fn) = geoid_undulation_m {
        let n = geoid_fn(lat_deg, lon_deg);
        Some(h_ellip_m - n)
    } else {
        None // without a geoid model, you can’t reliably estimate MSL
    }
}

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
