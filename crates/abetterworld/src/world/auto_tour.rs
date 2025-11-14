use crate::{CameraPosition, Location, Orientation};
use std::f64::consts::PI;
use std::time::Instant;

const EARTH_RADIUS_M: f64 = 6_371_000.0;

#[inline]
fn exp_smooth_towards(curr: f64, target: f64, rate_per_sec: f64, dt_s: f64) -> f64 {
    let a = (-rate_per_sec * dt_s.max(1e-4)).exp();
    curr * a + target * (1.0 - a)
}

#[inline]
fn clamp_lat(lat: f64) -> f64 {
    lat.clamp(-85.0, 85.0)
}
#[inline]
fn wrap_lon(mut lon: f64) -> f64 {
    while lon <= -180.0 {
        lon += 360.0;
    }
    while lon > 180.0 {
        lon -= 360.0;
    }
    lon
}
#[inline]
fn deg2rad(d: f64) -> f64 {
    d * PI / 180.0
}
#[inline]
fn rad2deg(r: f64) -> f64 {
    r * 180.0 / PI
}
#[inline]
fn normalize_bearing(mut b: f64) -> f64 {
    while b < 0.0 {
        b += 360.0;
    }
    while b >= 360.0 {
        b -= 360.0;
    }
    b
}
#[inline]
fn smoothstep01(x: f64) -> f64 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[derive(Debug, Clone, Copy)]
struct Waypoint {
    name: &'static str,
    lat_deg: f64,
    lon_deg: f64,
    low_alt_m: f64,
}

const WAYPOINTS: &[Waypoint] = &[
    Waypoint {
        name: "Grand Canyon, USA",
        lat_deg: 36.106965,
        lon_deg: -112.112997,
        low_alt_m: 3000.0,
    },
    Waypoint {
        name: "Eiffel Tower, Paris",
        lat_deg: 48.858370,
        lon_deg: 2.294481,
        low_alt_m: 900.0,
    },
    Waypoint {
        name: "Mount Everest",
        lat_deg: 27.988119,
        lon_deg: 86.925026,
        low_alt_m: 9500.0,
    },
    Waypoint {
        name: "Tokyo, Japan",
        lat_deg: 35.676200,
        lon_deg: 139.650311,
        low_alt_m: 1000.0,
    },
    Waypoint {
        name: "New York City, USA",
        lat_deg: 40.712776,
        lon_deg: -74.005974,
        low_alt_m: 900.0,
    },
    Waypoint {
        name: "Rio de Janeiro, Brazil",
        lat_deg: -22.906847,
        lon_deg: -43.172897,
        low_alt_m: 1100.0,
    },
    Waypoint {
        name: "Cape Town, South Africa",
        lat_deg: -33.924870,
        lon_deg: 18.424055,
        low_alt_m: 1200.0,
    },
    Waypoint {
        name: "Sydney Opera House",
        lat_deg: -33.856784,
        lon_deg: 151.215297,
        low_alt_m: 1000.0,
    },
    Waypoint {
        name: "Pyramids of Giza, Egypt",
        lat_deg: 29.979235,
        lon_deg: 31.134202,
        low_alt_m: 1200.0,
    },
    Waypoint {
        name: "Amazon (Manaus), Brazil",
        lat_deg: -3.119028,
        lon_deg: -60.021731,
        low_alt_m: 1600.0,
    },
    Waypoint {
        name: "Antarctica (McMurdo)",
        lat_deg: -77.841900,
        lon_deg: 166.686300,
        low_alt_m: 4000.0,
    },
];

#[derive(Debug, Clone, Copy)]
enum Phase {
    Cruise,
    Dive,
    Track,
    Climb,
}

/// Advance lat/lon by traveling distance `dist_m` from (lat,lon) on bearing `brg_deg` along a great circle.
fn step_great_circle(lat_deg: f64, lon_deg: f64, brg_deg: f64, dist_m: f64) -> (f64, f64) {
    let φ1 = deg2rad(lat_deg);
    let λ1 = deg2rad(lon_deg);
    let θ = deg2rad(brg_deg);
    let δ = dist_m / EARTH_RADIUS_M;

    let sinφ1 = φ1.sin();
    let cosφ1 = φ1.cos();
    let sinδ = δ.sin();
    let cosδ = δ.cos();
    let sinθ = θ.sin();
    let cosθ = θ.cos();

    let sinφ2 = sinφ1 * cosδ + cosφ1 * sinδ * cosθ;
    let φ2 = sinφ2.asin();

    let y = sinθ * sinδ * cosφ1;
    let x = cosδ - sinφ1 * sinφ2;
    let λ2 = λ1 + y.atan2(x);

    (clamp_lat(rad2deg(φ2)), wrap_lon(rad2deg(λ2)))
}

// ---------- Local-ENU helpers (for tiny horizon peek) ----------
#[inline]
fn geodetic_to_ecef(lat_deg: f64, lon_deg: f64, alt_m: f64) -> (f64, f64, f64) {
    let r = EARTH_RADIUS_M + alt_m;
    let lat = deg2rad(lat_deg);
    let lon = deg2rad(lon_deg);
    let clat = lat.cos();
    let slat = lat.sin();
    let clon = lon.cos();
    let slon = lon.sin();
    (r * clat * clon, r * clat * slon, r * slat)
}
#[inline]
fn enu_basis_ecef(
    lat_deg: f64,
    lon_deg: f64,
) -> ((f64, f64, f64), (f64, f64, f64), (f64, f64, f64)) {
    let lat = deg2rad(lat_deg);
    let lon = deg2rad(lon_deg);
    let (slat, clat) = (lat.sin(), lat.cos());
    let (slon, clon) = (lon.sin(), lon.cos());
    let e = (-slon, clon, 0.0);
    let n = (-slat * clon, -slat * slon, clat);
    let u = (clat * clon, clat * slon, slat);
    (e, n, u)
}
#[inline]
fn vadd(a: (f64, f64, f64), b: (f64, f64, f64)) -> (f64, f64, f64) {
    (a.0 + b.0, a.1 + b.1, a.2 + b.2)
}
#[inline]
fn vscale(a: (f64, f64, f64), s: f64) -> (f64, f64, f64) {
    (a.0 * s, a.1 * s, a.2 * s)
}
#[inline]
fn vlerp(a: (f64, f64, f64), b: (f64, f64, f64), t: f64) -> (f64, f64, f64) {
    vadd(vscale(a, 1.0 - t), vscale(b, t))
}

/// Build a tiny forward target (pitched up) in ECEF; we'll blend this with the nadir target (0,0,0).
fn forward_peek_target(
    lat_deg: f64,
    lon_deg: f64,
    alt_m: f64,
    heading_deg: f64,
    pitch_up_deg: f64,
    look_ahead_m: f64,
) -> (f64, f64, f64) {
    let (px, py, pz) = geodetic_to_ecef(lat_deg, lon_deg, alt_m);
    let (e, n, u) = enu_basis_ecef(lat_deg, lon_deg);

    let hdg = deg2rad(heading_deg);
    let pitch = deg2rad(pitch_up_deg.max(0.0));

    let cos_p = pitch.cos();
    let sin_p = pitch.sin();
    let cos_h = hdg.cos();
    let sin_h = hdg.sin();

    // forward ENU direction, slightly pitched up
    let dir = vadd(
        vadd(vscale(e, cos_p * cos_h), vscale(n, cos_p * sin_h)),
        vscale(u, sin_p),
    );
    let mut target = vadd((px, py, pz), vscale(dir, look_ahead_m));
    target
}

/// Deterministic tour. Dive/Climb are arrival-based. Cruise spins directly (no lag).
/// During Track we *blend* a small horizon peek (forward target) and then blend back to nadir.
pub struct AutoTour {
    // Time
    last_tick: Instant,
    phase_t: f64,
    phase: Phase,

    // Current geodetic pose
    lat_deg: f64,
    lon_deg: f64,
    alt_m: f64,

    // Targets we ease toward
    tgt_lat_deg: f64,
    tgt_lon_deg: f64,
    tgt_alt_m: f64,

    // Waypoints
    wp_index: usize,

    // Tunables
    cruise_alt_m: f64,
    cruise_time_s: f64,
    track_time_s: f64,

    // Motion feel
    cruise_turn_dps: f64,
    lat_lerp_rate: f64,
    lon_lerp_rate: f64,
    alt_lerp_rate: f64,

    // Track motion
    track_ground_speed_mps: f64,
    track_heading_deg: f64,
    track_heading_drift_dps: f64,

    // Track horizon-peek envelope
    track_peek_pitch_deg: f64, // small: 6–10°
    track_peek_rise_frac: f64, // fraction of Track to rise to full peek (e.g., 0.20)
    track_peek_fall_frac: f64, // fraction of Track to fall back to nadir at end (e.g., 0.25)
    track_peek_look_k: f64,    // look distance = k * alt (keeps peek subtle at low alt)

    // Arrival tolerances
    pos_tol_deg: f64,
    alt_tol_m: f64,

    // Climb spin ramp
    climb_spin_start_frac: f64,
    climb_spin_max_dps: f64,
}

impl AutoTour {
    pub fn new() -> Self {
        let start = Instant::now();
        let lat0 = 34.4459619;
        let lon0 = -119.666524;
        let cruise_alt = 4_000_000.0;

        let mut s = Self {
            last_tick: start,
            phase_t: 0.0,
            phase: Phase::Cruise,

            lat_deg: lat0,
            lon_deg: lon0,
            alt_m: cruise_alt,

            tgt_lat_deg: lat0,
            tgt_lon_deg: lon0,
            tgt_alt_m: cruise_alt,

            wp_index: 0,

            cruise_alt_m: cruise_alt,
            cruise_time_s: 4.0,
            track_time_s: 5.0,

            cruise_turn_dps: 6.0,
            lat_lerp_rate: 1.8,
            lon_lerp_rate: 2.0,
            alt_lerp_rate: 2.2,

            track_ground_speed_mps: 150.0,
            track_heading_deg: 90.0,
            track_heading_drift_dps: 3.0,

            // Subtle horizon peek defaults
            track_peek_pitch_deg: 8.0,
            track_peek_rise_frac: 0.20,
            track_peek_fall_frac: 0.25,
            track_peek_look_k: 1.2,

            pos_tol_deg: 0.02,
            alt_tol_m: 25.0,

            climb_spin_start_frac: 0.25,
            climb_spin_max_dps: 6.0,
        };
        s.sync_targets_to_current();
        s
    }

    fn current_wp(&self) -> Waypoint {
        WAYPOINTS[self.wp_index % WAYPOINTS.len()]
    }
    fn next_wp(&mut self) {
        self.wp_index = (self.wp_index + 1) % WAYPOINTS.len();
    }

    fn sync_targets_to_current(&mut self) {
        self.tgt_lat_deg = self.lat_deg;
        self.tgt_lon_deg = self.lon_deg;
        self.tgt_alt_m = self.alt_m;
    }

    fn begin_cruise(&mut self) {
        self.phase = Phase::Cruise;
        self.phase_t = 0.0;
        self.tgt_alt_m = self.cruise_alt_m;
        self.tgt_lon_deg = self.lon_deg; // keep targets aligned to avoid easing lag
        self.tgt_lat_deg = self.lat_deg;
    }

    fn begin_dive(&mut self) {
        self.phase = Phase::Dive;
        self.phase_t = 0.0;
        let wp = self.current_wp();
        self.tgt_lat_deg = clamp_lat(wp.lat_deg);
        self.tgt_lon_deg = wrap_lon(wp.lon_deg);
        self.tgt_alt_m = wp.low_alt_m;
        self.track_heading_deg =
            self.initial_bearing(self.lat_deg, self.lon_deg, wp.lat_deg, wp.lon_deg);
    }

    fn begin_track(&mut self) {
        self.phase = Phase::Track;
        self.phase_t = 0.0;
        self.tgt_alt_m = self.current_wp().low_alt_m;
        self.track_heading_deg = normalize_bearing(self.track_heading_deg);
        // IMPORTANT: no change to targets that would cause an immediate re-aim
        // The peek is blended purely in orientation, not by jumping the target.
    }

    fn begin_climb(&mut self) {
        self.phase = Phase::Climb;
        self.phase_t = 0.0;
        self.tgt_alt_m = self.cruise_alt_m;
    }

    fn initial_bearing(&self, lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
        let φ1 = deg2rad(lat1);
        let φ2 = deg2rad(lat2);
        let Δλ = deg2rad(lon2 - lon1);
        let y = Δλ.sin() * φ2.cos();
        let x = φ1.cos() * φ2.sin() - φ1.sin() * φ2.cos() * Δλ.cos();
        normalize_bearing(rad2deg(y.atan2(x)))
    }

    #[inline]
    fn near_latlon(&self, lat_t: f64, lon_t: f64) -> bool {
        (self.lat_deg - lat_t).abs() <= self.pos_tol_deg && {
            let mut d = self.lon_deg - lon_t;
            if d > 180.0 {
                d -= 360.0;
            }
            if d < -180.0 {
                d += 360.0;
            }
            d.abs() <= self.pos_tol_deg
        }
    }
    #[inline]
    fn near_alt(&self, alt_t: f64) -> bool {
        (self.alt_m - alt_t).abs() <= self.alt_tol_m
    }

    pub fn step(&mut self) -> Option<CameraPosition> {
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        let dt_s = dt.as_secs_f64().max(1e-4);
        self.phase_t += dt_s;

        match self.phase {
            Phase::Cruise => {
                // Direct spin (no lag)
                self.lon_deg = wrap_lon(self.lon_deg + self.cruise_turn_dps * dt_s);
                self.tgt_lon_deg = self.lon_deg;
                self.tgt_lat_deg = self.lat_deg;
                self.tgt_alt_m = self.cruise_alt_m;

                if self.phase_t >= self.cruise_time_s {
                    self.begin_dive();
                }
            }
            Phase::Dive => {
                let wp = self.current_wp();
                if self.near_latlon(wp.lat_deg, wp.lon_deg) && self.near_alt(wp.low_alt_m) {
                    self.begin_track();
                }
            }
            Phase::Track => {
                // Ground pan
                let dist_m = self.track_ground_speed_mps * dt_s;
                self.track_heading_deg =
                    normalize_bearing(self.track_heading_deg + self.track_heading_drift_dps * dt_s);

                let (nlat, nlon) = step_great_circle(
                    self.tgt_lat_deg,
                    self.tgt_lon_deg,
                    self.track_heading_deg,
                    dist_m,
                );
                self.tgt_lat_deg = clamp_lat(nlat);
                self.tgt_lon_deg = wrap_lon(nlon);
                self.tgt_alt_m = self.current_wp().low_alt_m;

                if self.phase_t >= self.track_time_s {
                    self.begin_climb();
                }
            }
            Phase::Climb => {
                // Spin ramp to remove pull-out hesitation
                let start_alt = self.cruise_alt_m * self.climb_spin_start_frac;
                let frac =
                    ((self.alt_m - start_alt) / (self.cruise_alt_m - start_alt)).clamp(0.0, 1.0);
                let spin_rate = self.climb_spin_max_dps * smoothstep01(frac);
                if spin_rate > 0.0 {
                    self.lon_deg = wrap_lon(self.lon_deg + spin_rate * dt_s);
                    self.tgt_lon_deg = self.lon_deg;
                }
                if self.near_alt(self.cruise_alt_m) {
                    self.next_wp();
                    self.begin_cruise();
                }
            }
        }

        // Ease toward targets (shortest-arc lon)
        let mut lon_target = self.tgt_lon_deg;
        let mut lon_curr = self.lon_deg;
        let d = lon_target - lon_curr;
        if d > 180.0 {
            lon_target -= 360.0;
        } else if d < -180.0 {
            lon_target += 360.0;
        }

        self.lat_deg = clamp_lat(exp_smooth_towards(
            self.lat_deg,
            self.tgt_lat_deg,
            self.lat_lerp_rate,
            dt_s,
        ));
        lon_curr = exp_smooth_towards(lon_curr, lon_target, self.lon_lerp_rate, dt_s);
        self.lon_deg = wrap_lon(lon_curr);
        self.alt_m = exp_smooth_towards(self.alt_m, self.tgt_alt_m, self.alt_lerp_rate, dt_s);

        // -------- Orientation --------
        // Base (nadir) target is Earth's center in ECEF.
        let nadir_target = (0.0, 0.0, 0.0);
        let up_vec; // local up at current pose
        {
            let (_e, _n, u) = enu_basis_ecef(self.lat_deg, self.lon_deg);
            up_vec = u;
        }

        // During Track, blend in a tiny forward target (horizon peek), then blend back to nadir.
        let target_ecef = if let Phase::Track = self.phase {
            // Envelope: rise over first track_peek_rise_frac, hold (if any), then fall over last track_peek_fall_frac
            let t = if self.track_time_s > 0.0 {
                (self.phase_t / self.track_time_s).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let rise = self.track_peek_rise_frac.clamp(0.01, 0.2);
            let fall = self.track_peek_fall_frac.clamp(0.01, 0.2);
            let fall_start = (1.0 - fall).max(rise);

            let env = if t < rise {
                // ease up 0 -> 1
                smoothstep01(t / rise)
            } else if t < fall_start {
                1.0
            } else {
                // ease down 1 -> 0
                let u = (t - fall_start) / (1.0 - fall_start);
                1.0 - smoothstep01(u)
            };

            // Look distance proportional to altitude (small at low alt)
            let look = (self.alt_m.max(100.0)) * self.track_peek_look_k * env;
            let peek_pitch = self.track_peek_pitch_deg * env;

            let forward_tgt = forward_peek_target(
                self.lat_deg,
                self.lon_deg,
                self.alt_m - 5000.0,
                self.track_heading_deg,
                peek_pitch,
                look,
            );

            // Blend between nadir (center) and forward_tgt; env already 0..1
            vlerp(nadir_target, forward_tgt, env)
        } else {
            nadir_target
        };

        let pos = CameraPosition {
            location: Location::Geodetic(self.lat_deg, self.lon_deg, self.alt_m),
            orientation: Orientation::TargetUp(target_ecef, up_vec),
        };
        Some(pos)
    }
}
