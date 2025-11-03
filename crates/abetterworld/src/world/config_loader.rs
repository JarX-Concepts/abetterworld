// world/config_loader.rs
use crate::world::{Config, Source}; // or the correct path
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadConfigError {
    #[error("config build error: {0}")]
    Build(#[from] config::ConfigError),
}

#[cfg(not(target_arch = "wasm32"))]
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
    log::info!("Config loaded successfully {:?}", cfg);

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

#[cfg(target_arch = "wasm32")]
pub fn load_config() -> Result<Config, LoadConfigError> {
    // return error
    Err(LoadConfigError::Build(config::ConfigError::Message(
        "Config loading not supported on wasm32".to_string(),
    )))
}
