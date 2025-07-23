use thiserror::Error;

#[derive(Debug, Error)]
pub enum AbwError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Tile loading failed: {0}")]
    TileLoading(String),

    #[error("Network failure: {0}")]
    Network(String),

    #[error("GPU error: {0}")]
    Gpu(#[from] wgpu::SurfaceError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Unexpected internal error: {0}")]
    Internal(String),
}

pub trait TileLoadingContext<T> {
    fn tile_loading(self, msg: &str) -> Result<T, AbwError>;
}

impl<T, E> TileLoadingContext<T> for Result<T, E>
where
    E: std::fmt::Display,
{
    fn tile_loading(self, msg: &str) -> Result<T, AbwError> {
        self.map_err(|e| AbwError::TileLoading(format!("{}: {}", msg, e)))
    }
}
