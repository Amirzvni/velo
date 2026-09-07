//! Velo download engine: probing, segmented transfer, resume.

pub mod engine;
pub mod error;
pub mod fsutil;
pub mod paths;
pub mod probe;
pub mod segment;
pub mod worker;
pub mod types;

pub use error::{Result, VeloError};
pub use types::*;

use std::time::Duration;

/// Shared client. One client = one connection pool = keep-alive reuse across
/// every segment, which is a large part of why this is fast.
pub fn build_client(user_agent: &str) -> Result<reqwest::Client> {
    let client = reqwest::Client::builder()
        .user_agent(user_agent.to_string())
        .pool_max_idle_per_host(64)
        .pool_idle_timeout(Duration::from_secs(90))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .tcp_nodelay(true)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;
    Ok(client)
}

pub const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36 Velo/0.1";
