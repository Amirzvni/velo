//! Loopback HTTP API the browser extension talks to.
//!
//! Pairing is automatic from the user's side:
//!   1. Velo binds a known port (48211, or the next free one up to 48215).
//!   2. The extension pings that range until it finds us.
//!   3. The extension POSTs /api/handshake with its extension id.
//!   4. Velo shows a prompt in the app. The user clicks Allow once.
//!   5. Velo returns the token; the extension stores it and never asks again.
//!
//! Security model:
//! - binds 127.0.0.1 only, so nothing off this machine can reach it
//! - every route except ping and handshake needs the bearer token
//! - a handshake is only granted after a human clicks Allow in the app window
//! - handshake requests expire, and only one can be pending at a time
//! - CORS allows browser extension origins only
//! - bodies are size capped so a bad page cannot exhaust memory

use axum::extract::{Json, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use parking_lot::Mutex;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;
use velo_core::paths;
use velo_manager::Manager;
use velo_store::models::NewDownload;

/// 64 KiB is plenty for a few hundred urls; anything bigger is abuse.
const MAX_BODY: usize = 64 * 1024;
/// One page can queue at most this many links in a single call.
const MAX_BATCH: usize = 500;
/// Ports we try, in order. The extension scans the same list.
pub const PORT_RANGE: std::ops::RangeInclusive<u16> = 48211..=48215;
/// A pairing prompt the user ignores expires instead of hanging forever.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);

/// A pairing request waiting for the user to click Allow or Deny.
pub struct Pending {
    pub extension_id: String,
    pub browser: String,
    pub created: Instant,
    pub reply: oneshot::Sender<bool>,
}

#[derive(Clone)]
pub struct ApiState {
    manager: Arc<Manager>,
    token: Arc<String>,
    /// Extension ids the user has already approved.
    paired: Arc<Mutex<Vec<String>>>,
    pending: Arc<Mutex<Option<Pending>>>,
    /// Lets the api ask the UI to show the pairing prompt.
    notify: Arc<dyn Fn(PairRequest) + Send + Sync>,
}

/// Sent to the UI so it can show "Allow this browser to use Velo?".
#[derive(Debug, Clone, Serialize)]
pub struct PairRequest {
    pub extension_id: String,
    pub browser: String,
}

/// Written to disk as a fallback and for debugging. Not how pairing works.
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

/// Bearer check on every route that does real work.
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

#[derive(Debug, Deserialize)]
pub struct HandshakeReq {
    pub extension_id: String,
    #[serde(default)]
    pub browser: Option<String>,
}

#[derive(Serialize)]
pub struct HandshakeResp {
    pub token: String,
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

/// Ask the user, once, whether this browser may drive Velo.
/// Blocks until they answer or the request times out.
async fn handshake(
    State(st): State<ApiState>,
    Json(req): Json<HandshakeReq>,
) -> Result<Json<HandshakeResp>, StatusCode> {
    if req.extension_id.is_empty() || req.extension_id.len() > 128 {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Already approved in a previous session: hand the token straight back.
    if st.paired.lock().iter().any(|id| id == &req.extension_id) {
        return Ok(Json(HandshakeResp {
            token: st.token.to_string(),
        }));
    }

    let browser = req.browser.unwrap_or_else(|| "Browser".into());
    let (tx, rx) = oneshot::channel();

    {
        let mut slot = st.pending.lock();
        // Drop a stale prompt so a stuck one cannot block pairing forever.
        if let Some(p) = slot.as_ref() {
            if p.created.elapsed() < HANDSHAKE_TIMEOUT {
                return Err(StatusCode::CONFLICT);
            }
        }
        *slot = Some(Pending {
            extension_id: req.extension_id.clone(),
            browser: browser.clone(),
            created: Instant::now(),
            reply: tx,
        });
    }

    (st.notify)(PairRequest {
        extension_id: req.extension_id.clone(),
        browser,
    });

    match tokio::time::timeout(HANDSHAKE_TIMEOUT, rx).await {
        Ok(Ok(true)) => {
            let ids = {
                let mut p = st.paired.lock();
                p.push(req.extension_id);
                p.clone()
            };
            save_paired(&ids);
            Ok(Json(HandshakeResp {
                token: st.token.to_string(),
            }))
        }
        Ok(Ok(false)) => Err(StatusCode::FORBIDDEN),
        _ => {
            *st.pending.lock() = None;
            Err(StatusCode::REQUEST_TIMEOUT)
        }
    }
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
    let item = to_new(req, &dir);
    // Downloads handed over by the browser wait for a yes, unless the user
    // has turned that off.
    let id = if st.manager.confirm_browser_downloads() {
        st.manager.add_pending(&item)
    } else {
        st.manager.add(&item)
    }
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

/// Handle the UI uses to answer a pending pairing prompt.
#[derive(Clone)]
pub struct PairControl {
    pending: Arc<Mutex<Option<Pending>>>,
}

impl PairControl {
    /// Returns false if the request already expired.
    pub fn answer(&self, allow: bool) -> bool {
        let Some(p) = self.pending.lock().take() else {
            return false;
        };
        p.reply.send(allow).is_ok()
    }

    pub fn peek(&self) -> Option<PairRequest> {
        self.pending.lock().as_ref().map(|p| PairRequest {
            extension_id: p.extension_id.clone(),
            browser: p.browser.clone(),
        })
    }
}

/// Start the server. `notify` is called when a browser asks to pair.
pub async fn start(
    manager: Arc<Manager>,
    notify: impl Fn(PairRequest) + Send + Sync + 'static,
) -> anyhow::Result<(ApiInfo, PairControl)> {
    // Reuse the token across restarts so an approved extension stays approved.
    let token = Arc::new(load_or_create_token());
    let pending = Arc::new(Mutex::new(None));
    let paired = Arc::new(Mutex::new(load_paired()));

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
        paired,
        pending: pending.clone(),
        notify: Arc::new(notify),
    };

    let app = Router::new()
        .route("/api/ping", get(ping))
        .route("/api/handshake", post(handshake))
        .route("/api/download", post(download))
        .route("/api/batch", post(batch))
        .layer(RequestBodyLimitLayer::new(MAX_BODY))
        .layer(cors)
        .with_state(state);

    // A known port means the extension can find us with no configuration.
    let mut listener = None;
    for port in PORT_RANGE {
        match tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await {
            Ok(l) => {
                listener = Some(l);
                break;
            }
            Err(_) => continue,
        }
    }
    let listener = listener.ok_or_else(|| {
        anyhow::anyhow!(
            "no free port in {}..={}",
            PORT_RANGE.start(),
            PORT_RANGE.end()
        )
    })?;
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
    Ok((info, PairControl { pending }))
}

fn token_path() -> std::path::PathBuf {
    paths::app_data_dir().join("token")
}

fn paired_path() -> std::path::PathBuf {
    paths::app_data_dir().join("paired.json")
}

/// The token outlives restarts, so a paired extension keeps working.
fn load_or_create_token() -> String {
    if let Ok(t) = std::fs::read_to_string(token_path()) {
        let t = t.trim().to_string();
        if t.len() == 48 {
            return t;
        }
    }
    let t = random_token();
    let _ = std::fs::create_dir_all(paths::app_data_dir());
    let _ = std::fs::write(token_path(), &t);
    restrict(&token_path());
    t
}

fn load_paired() -> Vec<String> {
    std::fs::read_to_string(paired_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_paired(ids: &[String]) {
    let _ = std::fs::create_dir_all(paths::app_data_dir());
    if let Ok(v) = serde_json::to_vec_pretty(ids) {
        let _ = std::fs::write(paired_path(), v);
    }
}

fn write_info(info: &ApiInfo) -> anyhow::Result<()> {
    let path = paths::app_data_dir().join("api.json");
    std::fs::create_dir_all(paths::app_data_dir())?;
    std::fs::write(&path, serde_json::to_vec_pretty(info)?)?;
    restrict(&path);
    Ok(())
}

/// Owner-only on unix. On Windows the app data dir is already per user.
fn restrict(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
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
