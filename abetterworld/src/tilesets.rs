use crate::cache::get_tileset_cache;
use crate::errors::{AbwError, TileLoadingContext};
use crate::Camera;
use bytes::Bytes;
use reqwest::blocking::Client;
use serde::Deserialize;
use url::Url;

const GOOGLE_API_KEY: &str = "AIzaSyDrSNqujmAmhhZtenz6MEofEuITd3z0JM0";
const GOOGLE_API_URL: &str = "https://tile.googleapis.com/v1/3dtiles/root.json";

#[derive(Debug, Deserialize)]
struct GltfTileset {
    root: GltfTile,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq)]
pub struct BoundingVolume {
    #[serde(rename = "box")]
    bounding_box: [f64; 12],
}

#[derive(Debug, Deserialize, Clone)]
pub struct GltfTileContent {
    uri: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GltfTile {
    #[serde(rename = "boundingVolume")]
    bounding_volume: BoundingVolume,
    #[serde(rename = "geometricError")]
    geometric_error: f64,
    refine: Option<String>,
    content: Option<GltfTileContent>,
    children: Option<Vec<GltfTile>>,
}

#[derive(Debug, Clone)]
pub struct TileSetImporter {
    client: Client,
    url: String,
    key: String,
}

fn resolve_url(base: &str, relative: &str) -> Result<String, AbwError> {
    use url::Url;
    let mut base_url = Url::parse(base).tile_loading("invalid base url")?;
    base_url.set_query(None);
    Ok(if relative.starts_with("http") {
        relative.to_string()
    } else {
        base_url
            .join(relative)
            .tile_loading("Failed to join base url")?
            .to_string()
    })
}

fn is_nested_tileset(uri: &str) -> bool {
    Url::parse(uri)
        .map(|url| url.path().ends_with(".json"))
        .unwrap_or_else(|_| {
            uri.split('?')
                .next()
                .map_or(false, |path| path.ends_with(".json"))
        })
}

fn is_glb(uri: &str) -> bool {
    uri.trim_end_matches('/').ends_with(".glb")
}

fn extract_session(url: &str) -> Option<&str> {
    url.split_once("session=").map(|(_, session)| session)
}

impl TileSetImporter {
    pub fn new(client: &Client, key: &str, url: &str) -> Self {
        Self {
            client: client.clone(),
            key: key.to_string(),
            url: url.to_string(),
        }
    }

    pub fn go(self, camera: &Camera) -> Result<(), AbwError> {
        return self.import_tileset(camera, self.url.as_str(), None);
    }

    fn import_tileset(
        &self,
        camera: &Camera,

        url: &str,
        session: Option<&str>,
    ) -> Result<(), AbwError> {
        let (content_type, bytes) = self.download_content(url, &self.key, session)?;

        match content_type.as_str() {
            "application/json" | "application/json; charset=UTF-8" => {
                let tileset: GltfTileset =
                    serde_json::from_slice(&bytes).tile_loading("Failed to parse tileset JSON")?;

                self.process_tileset(camera, &tileset.root, url, &self.key, session)
            }
            _ => Err(AbwError::TileLoading(
                format!(
                    "Unsupported content type: {} for {} ({})",
                    content_type, url, self.key
                )
                .into(),
            )),
        }
    }

    fn process_tileset(
        &self,
        camera: &Camera,
        tile_info: &GltfTile,
        tileset_url: &str,
        key: &str,
        session: Option<&str>,
    ) -> Result<(), AbwError> {
        let mut added_geom = false;

        let needs_refinement = camera.needs_refinement(
            &tile_info.bounding_volume,
            tile_info.geometric_error,
            1024.0,
            100.0,
        );

        if let Some(content) = &tile_info.content {
            let tile_url = resolve_url(&tileset_url, &content.uri)?;
            let refine_mode = tile_info.refine.as_deref().unwrap_or("REPLACE");
            let new_session = extract_session(&tile_url).or_else(|| session);

            if is_nested_tileset(&tile_url) {
                let nested = self.import_tileset(camera, &tile_url, new_session)?;
            } else if is_glb(&tile_url)
                && (refine_mode == "ADD" || tile_info.children.is_none() || !needs_refinement)
            {
                added_geom = true;
                // push this tile into the next thread...
                // use std::sync::mpsc::channel
            } else if !is_glb(&tile_url) {
                // these are weird
                // https://tile.googleapis.com/v1/3dtiles/datasets/CgIYAQ/files/AJVsH2xhxJPWKbFgSv4QaTrl7SbTaFlJnvfES7rtU4UHj6Lt5ys_EykyPb_P6NdvvMm8XTjWA6bKUyTq94uFkec53CIZF33frCoSLMBSQiOnIlPsKc0G8BsSlYvL.glb?session=CPecuaT_6-PRSBDl8uHDBg
                // log::info!("What is this tile {}", tile_url);
            }
        }

        if needs_refinement || !added_geom {
            if let Some(children) = &tile_info.children {
                for child in children {
                    self.process_tileset(camera, &child, &tileset_url, &key, session)?;
                }
            }
        }

        Ok(())
    }

    fn download_content(
        &self,
        content_url: &str,
        key: &str,
        session: Option<&str>,
    ) -> Result<(String, Bytes), AbwError> {
        // Try cache first
        if let Some(cache) = get_tileset_cache() {
            if let Some((content_type, bytes)) = cache.get(content_url) {
                return Ok((content_type, bytes));
            }
        }

        let mut query_params = vec![("key", key)];

        if let Some(session) = session.as_deref() {
            query_params.push(("session", session));
        }

        let response = self
            .client
            .get(content_url)
            .query(&query_params)
            .send()
            .tile_loading(&format!("Failed to download content from {}", content_url))?;

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let expected_len = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<usize>().ok());

        let bytes = response.bytes().tile_loading(&format!(
            "Failed to access byte content from {}",
            content_url
        ))?;

        if let Some(expected) = expected_len {
            if bytes.len() < expected {
                log::error!(
                    "Truncated content: expected {} bytes, got {}",
                    expected,
                    bytes.len()
                );
            }
        }

        if let Some(cache) = get_tileset_cache() {
            cache.insert(content_url.to_string(), content_type.clone(), bytes.clone());
        }

        Ok((content_type, bytes))
    }
}
