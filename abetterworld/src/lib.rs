mod cache;
mod content;
mod decode;
mod dynamics;
mod helpers;
mod render;
mod world;

#[cfg(test)]
mod tests;

pub use world::{Config, InputEvent, Key, MouseButton, Source, World};

pub fn get_debug_config() -> Config {
    Config {
        source: Source::Google {
            key: "AIzaSyD526Czd1rD44BZE2d2R70-fBEdDdf6vZQ".to_string(),
            url: "https://tile.googleapis.com/v1/3dtiles/root.json".to_string(),
        },
        geodetic_position: (34.4208, -119.6982, 6_378_137.0 * 2.0), // Santa Barbara
        cache_dir: "../tilesets".to_string(),
        use_debug_camera: true,
        debug_camera_geodetic_position: (34.4208, -119.6982, 50000.0),
        debug_camera_render_frustum: true,
        debug_render_volumes: true,
        tile_culling: false,
    }
}
