// world/mod.rs (or a new config.rs module)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Geodetic {
    pub lon: f64,
    pub lat: f64,
    pub alt_m: f64,
}

impl From<(f64, f64, f64)> for Geodetic {
    fn from(t: (f64, f64, f64)) -> Self {
        Self {
            lat: t.0,
            lon: t.1,
            alt_m: t.2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Source {
    Google {
        key: String,
        url: String,
    },
    CesiumIon {
        key: String,
        url: String,
    },
    SelfHosted {
        headers: Vec<(String, String)>,
        url: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub source: Source,
    pub geodetic_position: Geodetic,
    pub cache_dir: String,
    pub use_debug_camera: bool,
    pub debug_camera_geodetic_position: Geodetic,
    pub debug_camera_render_frustum: bool,
    pub debug_render_volumes: bool,
    pub tile_culling: bool,
}
