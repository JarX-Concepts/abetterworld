mod cache;
mod content;
mod decode;
mod dynamics;
mod helpers;
mod render;
mod world;

#[cfg(test)]
mod tests;

pub use world::{
    AutoTour, CameraPosition, Config, InputEvent, Key, Location, MouseButton, Orientation, Source,
    World,
};

use crate::world::load_config;

pub fn get_debug_config() -> Config {
    load_config().unwrap_or_else(|_| Config {
        source: Source::Google {
            key: std::env::var("GOOGLE_MAPS_API_KEY").unwrap_or_default(),
            url: "https://tile.googleapis.com/v1/3dtiles/root.json".to_string(),
        },
        geodetic_position: (34.4208, -119.6982, 6_378_137.0 * 2.0).into(), // Santa Barbara
        cache_dir: "./tilesets".to_string(),
        use_debug_camera: false,
        debug_camera_geodetic_position: (34.4208, -119.6982, 500.0).into(),
        debug_camera_render_frustum: true,
        debug_render_volumes: false,
        debug_auto_tour: false,
        tile_culling: false,
    })
}
