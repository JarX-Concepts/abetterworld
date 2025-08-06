use bytes::Bytes;
use std::sync::Arc;

use crate::{
    cache::get_tileset_cache,
    content::Client,
    helpers::{AbwError, TileLoadingContext},
};

pub async fn download_content(
    client: &Client,
    content_url: &str,
    key: &str,
    session: &Option<Arc<String>>,
) -> Result<(String, Bytes), AbwError> {
    // Try cache first
    let cache = get_tileset_cache();
    if let Some((content_type, bytes)) = cache.get(content_url).await? {
        return Ok((content_type, bytes));
    }

    log::info!("Downloading content from {}", content_url);

    let mut query_params = vec![("key", key)];

    if let Some(session) = session.as_deref() {
        query_params.push(("session", session.as_str()));
    }

    let response_result = client
        .get(content_url)
        .query(&query_params)
        .send()
        .await
        .tile_loading(&format!("Failed to download content from {}", content_url));

    if let Err(e) = &response_result {
        log::error!("Failed to download content from {}: {:?}", content_url, e);
        return Err(AbwError::Network(format!(
            "Failed to download content from {}: {:?}",
            content_url, e
        )));
    }

    let response = response_result.unwrap();

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

    let bytes = response.bytes().await.tile_loading(&format!(
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

    let cache = get_tileset_cache();
    cache
        .insert(content_url.to_string(), content_type.clone(), bytes.clone())
        .await?;

    Ok((content_type, bytes))
}
