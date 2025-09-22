use abetterworld::{CameraPosition, Location, Orientation};
use std::time::{Duration, Instant};

/// Simple per-frame auto-zoom script for debug use.
pub struct AutoZoom {
    last_tick: Instant,
    lat_deg: f64,
    lon_deg: f64,
    alt_m: f64,
    /// When alt <= stop_alt_m, script finishes.
    stop_alt_m: f64,
    /// Exponential decay rate (per second). Bigger = faster.
    decay_per_sec: f64,
}

impl AutoZoom {
    pub fn new() -> Self {
        Self {
            last_tick: Instant::now(),
            lat_deg: 34.4459619,  // equator
            lon_deg: -119.666524, // prime meridian
            alt_m: 20_000_000.0,  // ~20,000 km up to see full globe nicely
            stop_alt_m: 500.0,    // “close to ground” threshold
            decay_per_sec: 0.85,  // tune to taste (0.5–1.5 are reasonable)
        }
    }

    /// Advance one frame. Returns `Some(CameraPosition)` while running, `None` when finished.
    pub fn step(&mut self) -> Option<CameraPosition> {
        // dt
        let now = Instant::now();
        let dt = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        let dt_s = dt.as_secs_f64().max(1e-4);

        // Exponential decay of altitude: alt(t+dt) = alt(t) * exp(-k * dt)
        let k = self.decay_per_sec;
        self.alt_m = (self.alt_m * (-k * dt_s).exp()).max(self.stop_alt_m);

        // Build a nadir-looking orientation (straight down) at current lat/lon.
        // Adjust these to match your types/handedness:
        //
        // - heading_deg: arbitrary (keep steady due N = 0)
        // - pitch_deg:   -90 means looking straight down in many systems
        // - roll_deg:    0 (no bank)
        //
        // If your Orientation uses quaternions, replace with your "look_at" or "nadir" ctor.
        let pos = CameraPosition {
            location: Location::Geodetic(self.lat_deg, self.lon_deg, self.alt_m),
            orientation: Orientation::TargetUp((0.0, 0.0, 0.0), (0.0, 0.0, 1.0)),
        };

        if self.alt_m <= self.stop_alt_m + 1e-3 {
            None
        } else {
            Some(pos)
        }
    }
}
