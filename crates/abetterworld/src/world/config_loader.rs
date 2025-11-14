// world/config_loader.rs
use crate::world::{Config, Source}; // or the correct path
use thiserror::Error;
use tracing::{event, Level};

#[derive(Debug, Error)]
pub enum LoadConfigError {
    #[error("config build error: {0}")]
    Build(#[from] config::ConfigError),

    #[error("config deserialize error: {0}")]
    De(#[from] serde_json::Error),
}

#[cfg(not(any(target_arch = "wasm32", target_os = "ios")))]
pub fn load_config() -> Result<Config, LoadConfigError> {
    let _ = dotenvy::dotenv();

    let builder = config::Config::builder()
        .add_source(config::File::with_name("abw").required(false))
        .add_source(config::File::with_name("abw.local").required(false))
        .add_source(
            config::Environment::with_prefix("ABW")
                .separator("__")
                .try_parsing(true)
                .list_separator(","),
        );

    let cfg = builder.build()?;
    event!(Level::INFO, "Config loaded successfully {:?}", cfg);

    let mut cfg: Config = cfg.try_deserialize()?;

    if let Source::Google { key, .. } = &mut cfg.source {
        if key.is_empty() {
            if let Ok(k) = std::env::var("GOOGLE_MAPS_API_KEY") {
                *key = k;
            }
        }
    }

    Ok(cfg)
}

#[cfg(any(target_arch = "wasm32", target_os = "ios"))]
pub fn load_config() -> Result<Config, LoadConfigError> {
    event!(tracing::Level::INFO, "Loading embedded config for wasm/ios");
    const CONFIG_JSON: &str = include_str!("../../../../abw_wasm_config.json");

    // Parse into your typed Config
    let cfg: Config = serde_json::from_str(CONFIG_JSON)?;

    Ok(cfg)
}
