use abetterworld::{CameraPosition, Location, Orientation};
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
        low_alt_m: 1200.0,
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
        low_alt_m: 2500.0,
    },
    Waypoint {
        name: "Great Barrier Reef",
        lat_deg: -18.287067,
        lon_deg: 147.699219,
        low_alt_m: 1800.0,
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
    let δ = dist_m / EARTH_RADIUS_M; // angular distance

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

/// Deterministic camera tour; Track is now a gentle ground pan along a great circle.
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
    dive_time_s: f64,
    track_time_s: f64,
    climb_time_s: f64,

    // Motion feel
    cruise_turn_dps: f64,
    lat_lerp_rate: f64,
    lon_lerp_rate: f64,
    alt_lerp_rate: f64,

    // Track-phase ground motion
    track_ground_speed_mps: f64,  // ~slow aircraft/helicopter pace
    track_heading_deg: f64,       // current ground heading during Track
    track_heading_drift_dps: f64, // gentle curve to avoid straight lines forever
}

impl AutoTour {
    pub fn new() -> Self {
        let start = Instant::now();
        let lat0 = 34.4459619;
        let lon0 = -119.666524;
        let cruise_alt = 20_000_000.0;

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
            cruise_time_s: 8.0,
            dive_time_s: 8.0,
            track_time_s: 12.0,
            climb_time_s: 6.0,

            cruise_turn_dps: 6.0,
            lat_lerp_rate: 1.8,
            lon_lerp_rate: 2.0,
            alt_lerp_rate: 2.2,

            track_ground_speed_mps: 150.0, // ≈ 300 kt is ~155 m/s; this is a nice “slow pan”
            track_heading_deg: 90.0,       // east by default; updated on each Track begin
            track_heading_drift_dps: 3.0,  // very gentle curving
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
    }

    fn begin_dive(&mut self) {
        self.phase = Phase::Dive;
        self.phase_t = 0.0;
        let wp = self.current_wp();
        self.tgt_lat_deg = clamp_lat(wp.lat_deg);
        self.tgt_lon_deg = wrap_lon(wp.lon_deg);
        self.tgt_alt_m = wp.low_alt_m;
        // Set a reasonable initial track heading: approach direction (bearing from current to wp)
        self.track_heading_deg =
            self.initial_bearing(self.lat_deg, self.lon_deg, wp.lat_deg, wp.lon_deg);
    }

    fn begin_track(&mut self) {
        self.phase = Phase::Track;
        self.phase_t = 0.0;
        self.tgt_alt_m = self.current_wp().low_alt_m;
        // Keep whatever heading we had on approach; start panning forward.
        self.track_heading_deg = normalize_bearing(self.track_heading_deg);
    }

    fn begin_climb(&mut self) {
        self.phase = Phase::Climb;
        self.phase_t = 0.0;
        self.tgt_alt_m = self.cruise_alt_m;
    }

    /// Bearing from (lat1,lon1) to (lat2,lon2) (degrees, 0°=north, clockwise east).
    fn initial_bearing(&self, lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
        let φ1 = deg2rad(lat1);
        let φ2 = deg2rad(lat2);
        let Δλ = deg2rad(lon2 - lon1);
        let y = Δλ.sin() * φ2.cos();
        let x = φ1.cos() * φ2.sin() - φ1.sin() * φ2.cos() * Δλ.cos();
        normalize_bearing(rad2deg(y.atan2(x)))
    }

    /// Call every frame; always returns Some(CameraPosition) to keep touring.
    pub fn step(&mut self) -> Option<CameraPosition> {
        // dt
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        let dt_s = dt.as_secs_f64().max(1e-4);
        self.phase_t += dt_s;

        // Phase behavior
        match self.phase {
            Phase::Cruise => {
                // Gentle turn at altitude to keep motion alive
                self.tgt_lon_deg = wrap_lon(self.tgt_lon_deg + self.cruise_turn_dps * dt_s);

                if self.phase_t >= self.cruise_time_s {
                    self.begin_dive();
                }
            }
            Phase::Dive => {
                // Ease toward waypoint at low altitude; time-box the descent
                if self.phase_t >= self.dive_time_s {
                    self.begin_track();
                }
            }
            Phase::Track => {
                // Move the TARGET point forward along the surface at constant ground speed.
                // This gives a smooth, ground-parallel pan when the camera eases toward it.
                let dist_m = self.track_ground_speed_mps * dt_s;
                // small heading drift to curve a bit
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
                if self.phase_t >= self.climb_time_s {
                    self.next_wp();
                    self.begin_cruise();
                }
            }
        }

        // Ease current pose toward targets — shortest-arc lon easing:
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

        // Nadir orientation (swap in your local-up look_at if you’ve got it handy)
        let pos = CameraPosition {
            location: Location::Geodetic(self.lat_deg, self.lon_deg, self.alt_m),
            orientation: Orientation::TargetUp((0.0, 0.0, 0.0), (0.0, 0.0, 1.0)),
        };

        Some(pos)
    }
}
