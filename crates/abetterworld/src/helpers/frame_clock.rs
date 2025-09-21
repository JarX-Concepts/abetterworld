// frame_clock.rs
#![allow(dead_code)]

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::{Duration, Instant};
#[cfg(target_arch = "wasm32")]
pub use web_time::{Duration, Instant};

/// Info returned each frame.
#[derive(Debug, Clone, Copy)]
pub struct Tick {
    /// Unclamped delta between this frame and the previous one.
    pub dt: Duration,
    /// Delta in seconds (f32) after clamping, convenient for math.
    pub dt_seconds: f32,
    /// Smoothed FPS (EMA).
    pub fps: f32,
    /// Total elapsed since the clock was created/reset.
    pub elapsed: Duration,
    /// Incrementing frame index (starts at 1 on first tick).
    pub frame: u64,
}

/// Cross-platform frame timing.
/// Call `tick()` once per frame/update to get timing info.
pub struct FrameClock {
    last: Instant,
    start: Instant,
    frame: u64,
    elapsed: Duration,
    ema_fps: f64,
    /// Clamp excessively large dt (e.g., after a pause) to avoid sim explosions.
    clamp_dt: Duration,
    /// EMA smoothing factor for FPS (0..1). Higher = snappier but noisier.
    fps_ema_alpha: f64,
}

impl FrameClock {
    /// Create a new clock.
    /// - `clamp_dt` e.g. 1/10s (100ms) keeps physics stable after long stalls.
    /// - `fps_ema_alpha` e.g. 0.2 is a reasonable default.
    pub fn new(clamp_dt: Duration, fps_ema_alpha: f64) -> Self {
        let now = Instant::now();
        Self {
            last: now,
            start: now,
            frame: 0,
            elapsed: Duration::ZERO,
            ema_fps: 60.0, // start with a sensible guess
            clamp_dt,
            fps_ema_alpha: fps_ema_alpha.clamp(0.0, 1.0),
        }
    }

    /// Convenience: 100ms clamp and 0.2 EMA.
    pub fn default() -> Self {
        Self::new(Duration::from_millis(100), 0.2)
    }

    /// Reset the clock as if just created.
    pub fn reset(&mut self) {
        let now = Instant::now();
        self.last = now;
        self.start = now;
        self.frame = 0;
        self.elapsed = Duration::ZERO;
        self.ema_fps = 60.0;
    }

    /// Advance one frame/update. Call this once per loop iteration.
    pub fn tick(&mut self) -> Tick {
        let now = Instant::now();
        let raw_dt = now.saturating_duration_since(self.last);
        self.last = now;

        // Clamp dt for simulation stability.
        let dt_clamped = if raw_dt > self.clamp_dt {
            self.clamp_dt
        } else {
            raw_dt
        };

        self.frame = self.frame.saturating_add(1);
        self.elapsed = now.saturating_duration_since(self.start);

        // Compute instantaneous fps from unclamped dt (reflects reality),
        // then smooth it for display/metrics.
        let inst_fps = if raw_dt.as_secs_f64() > 0.0 {
            1.0 / raw_dt.as_secs_f64()
        } else {
            self.ema_fps.max(1.0)
        };
        self.ema_fps = self.fps_ema_alpha * inst_fps + (1.0 - self.fps_ema_alpha) * self.ema_fps;

        Tick {
            dt: raw_dt,
            dt_seconds: dt_clamped.as_secs_f32(),
            fps: self.ema_fps as f32,
            elapsed: self.elapsed,
            frame: self.frame,
        }
    }
}
