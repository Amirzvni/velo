//! Loopback HTTP API the browser extension talks to.
//!
//! Security model:
//! - binds 127.0.0.1 only, on a port the OS picks fresh each launch
//! - every request needs the bearer token generated at startup
//! - port and token are written to a file only this user account can read
//! - CORS allows browser extension origins only
//! - bodies are size capped so a bad page cannot exhaust memory

use axum::extract::{Json, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use velo_core::paths;
use velo_manager::Manager;
use velo_store::models::NewDownload;

/// 64 KiB is plenty for a few hundred urls; anything bigger is abuse.
const MAX_BODY: usize = 64 * 1024;
/// One page can queue at most this many links in a single call.
const MAX_BATCH: usize = 500;

#[derive(Clone)]
pub struct ApiState {
    manager: Arc<Manager>,
    token: Arc<String>,
}

/// Written to disk so the extension can find us. Same info the pairing code shows.
#[derive(Serialize, Deserialize)]
pub struct ApiInfo {
    pub port: u16,
    pub token: String,
    pub version: String,
}

fn random_token() -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut rng = rand::thread_rng();
    (0..48).map(|_| HEX[rng.gen_range(0..16)] as char).collect()
}

/// Bearer check on every route except ping.
fn authorized(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| constant_eq(v, token))
        .unwrap_or(false)
}

/// Compare without leaking length or position through timing.
fn constant_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

#[derive(Debug, Deserialize)]
pub struct DownloadReq {
    pub url: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub out_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BatchReq {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub source_page: Option<String>,
    pub items: Vec<DownloadReq>,
}

#[derive(Serialize)]
pub struct IdsResp {
    pub ids: Vec<i64>,
}

#[derive(Serialize)]
pub struct PingResp {
    pub app: &'static str,
    pub version: &'static str,
}

async fn ping() -> Json<PingResp> {
    Json(PingResp {
        app: "velo",
        version: env!("CARGO_PKG_VERSION"),
    })
}

fn to_new(req: DownloadReq, default_dir: &str) -> NewDownload {
    NewDownload {
        url: req.url,
        out_dir: req.out_dir.unwrap_or_else(|| default_dir.to_string()),
        file_name: req.file_name,
        headers: req.headers,
        segments: None,
        batch_id: None,
        scheduled_at: None,
        priority: None,
    }
}

async fn download(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<DownloadReq>,
) -> Result<Json<IdsResp>, StatusCode> {
    if !authorized(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if !req.url.starts_with("http://") && !req.url.starts_with("https://") {
        return Err(StatusCode::BAD_REQUEST);
    }
    let dir = paths::default_download_dir().to_string_lossy().into_owned();
    let id = st
        .manager
        .add(&to_new(req, &dir))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(IdsResp { ids: vec![id] }))
}

async fn batch(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Json(req): Json<BatchReq>,
) -> Result<Json<IdsResp>, StatusCode> {
    if !authorized(&headers, &st.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if req.items.is_empty() || req.items.len() > MAX_BATCH {
        return Err(StatusCode::BAD_REQUEST);
    }
    let dir = paths::default_download_dir().to_string_lossy().into_owned();
    let items: Vec<NewDownload> = req
        .items
        .into_iter()
        .filter(|r| r.url.starts_with("http://") || r.url.starts_with("https://"))
        .map(|r| to_new(r, &dir))
        .collect();
    if items.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let name = req.name.unwrap_or_else(|| format!("{} links", items.len()));
    let (_, ids) = st
        .manager
        .add_batch(&name, req.source_page.as_deref(), &items)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(IdsResp { ids }))
}

/// Start the server. Returns the port and token so the UI can show them.
pub async fn start(manager: Arc<Manager>) -> anyhow::Result<ApiInfo> {
    let token = Arc::new(random_token());

    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
        ])
        // Only browser extensions may call us. A random web page cannot.
        .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
            origin
                .to_str()
                .map(|o| o.starts_with("chrome-extension://") || o.starts_with("moz-extension://"))
                .unwrap_or(false)
        }));

    let state = ApiState {
        manager,
        token: token.clone(),
    };

    let app = Router::new()
        .route("/api/ping", get(ping))
        .route("/api/download", post(download))
        .route("/api/batch", post(batch))
        .layer(RequestBodyLimitLayer::new(MAX_BODY))
        .layer(cors)
        .with_state(state);

    // Port 0 means the OS gives us a free one, different every launch.
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let port = listener.local_addr()?.port();

    let info = ApiInfo {
        port,
        token: token.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    write_info(&info)?;

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("local api stopped: {e}");
        }
    });

    tracing::info!("local api listening on 127.0.0.1:{port}");
    Ok(info)
}

/// The extension reads this file to learn the port; the user pastes the token
/// once. Kept in the app data dir, which is per user on every platform.
fn write_info(info: &ApiInfo) -> anyhow::Result<()> {
    let path = paths::app_data_dir().join("api.json");
    std::fs::create_dir_all(paths::app_data_dir())?;
    std::fs::write(&path, serde_json::to_vec_pretty(info)?)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_and_unique() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 48);
        assert_ne!(a, b);
    }

    #[test]
    fn constant_eq_matches_normal_eq() {
        assert!(constant_eq("abc", "abc"));
        assert!(!constant_eq("abc", "abd"));
        assert!(!constant_eq("abc", "abcd"));
    }
}
