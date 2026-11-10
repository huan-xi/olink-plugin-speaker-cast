//! speaker_cast 服务型插件进程：UDS 管理 API + Hello；LAN HTTP 直播由 Bridge 自绑。

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use http_body_util::Full;
use hyper::Request;
use hyper_util::rt::TokioIo;
use log::info;
use olink_plugin_speaker_cast::api;
use olink_plugin_speaker_cast::bootstrap::build_service;
use serde_json::{json, Value};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;

static SHUTDOWN: OnceLock<Arc<Notify>> = OnceLock::new();

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(
        env::var("OLINK_PLUGIN_LOG_LEVEL").unwrap_or_else(|_| "info".into()),
    ))
    .try_init();

    let sock = env::var("OLINK_PLUGIN_SOCK")?;
    let host_sock = env::var("OLINK_PLUGIN_HOST_SOCK")?;
    let token = env::var("OLINK_PLUGIN_TOKEN")?;
    let domain = env::var("OLINK_PLUGIN_DOMAIN").unwrap_or_else(|_| "speaker_cast".into());
    let version = env::var("OLINK_PLUGIN_VERSION").unwrap_or_else(|_| "1.0.0".into());
    let data_dir = PathBuf::from(env::var("OLINK_PLUGIN_DATA_DIR")?);

    let path = PathBuf::from(&sock);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }
    let _ = fs::remove_file(&path);

    let service = build_service(&data_dir).await?;
    api::set_service(Arc::clone(&service));
    service.restore().await?;
    info!("speaker_cast service ready");

    let shutdown = Arc::new(Notify::new());
    let _ = SHUTDOWN.set(Arc::clone(&shutdown));

    let app = Router::new()
        .route("/health", get(|| async { axum::http::StatusCode::OK }))
        .route("/olink/v1/shutdown", post(shutdown_handler))
        .merge(api::router());

    let listener = UnixListener::bind(&path)?;
    let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    let serve = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    tokio::time::sleep(Duration::from_millis(30)).await;
    post_hello(&host_sock, &token, &domain, &version).await?;
    info!("speaker_cast hello ok domain={domain}");

    shutdown.notified().await;
    serve.abort();
    let _ = fs::remove_file(&path);
    Ok(())
}

async fn shutdown_handler() -> Json<Value> {
    if let Some(n) = SHUTDOWN.get() {
        n.notify_waiters();
    }
    Json(json!({ "ok": true }))
}

async fn post_hello(host_sock: &str, token: &str, domain: &str, version: &str) -> anyhow::Result<()> {
    let body = json!({
        "domain": domain,
        "plugin_version": version,
        "protocol": 1,
        "features": ["log"],
        "features_required": [],
    });
    let (status, text) = uds_json(host_sock, "POST", "/olink/v1/hello", token, Some(&body)).await?;
    if !(200..300).contains(&status) {
        anyhow::bail!("hello status={status} body={text}");
    }
    Ok(())
}

async fn uds_json(
    sock: &str,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> anyhow::Result<(u16, String)> {
    let stream = UnixStream::connect(sock).await?;
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let bytes = match body {
        Some(b) => Bytes::from(serde_json::to_vec(b)?),
        None => Bytes::new(),
    };
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost")
        .header("X-Plugin-Token", token);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let req = builder.body(Full::new(bytes))?;
    let resp = sender.send_request(req).await?;
    let status = resp.status().as_u16();
    let collected = http_body_util::BodyExt::collect(resp.into_body()).await?;
    let text = String::from_utf8_lossy(&collected.to_bytes()).into_owned();
    Ok((status, text))
}
