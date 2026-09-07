use std::io;

#[derive(Debug, thiserror::Error)]
pub enum VeloError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("invalid url: {0}")]
    InvalidUrl(String),

    #[error("server returned status {0}")]
    BadStatus(u16),

    /// Server told us to slow down. Carries Retry-After seconds when given.
    #[error("rate limited (retry after {0:?}s)")]
    RateLimited(Option<u64>),

    #[error("unsafe file name: {0}")]
    UnsafeFileName(String),

    #[error("download was cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, VeloError>;

impl VeloError {
    /// Worth trying again on a fresh connection.
    pub fn is_retryable(&self) -> bool {
        match self {
            VeloError::Http(e) => e.is_timeout() || e.is_connect() || e.is_request(),
            VeloError::Io(_) => true,
            VeloError::RateLimited(_) => true,
            VeloError::BadStatus(s) => *s >= 500 || *s == 408,
            _ => false,
        }
    }
}
