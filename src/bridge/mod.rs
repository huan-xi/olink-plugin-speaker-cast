//! 出声桥：PCM → ffmpeg MP3 → HTTP 直播，供小爱 play_url 拉取。

use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use bytes::Bytes;
use futures::StreamExt;
use log::{info, warn};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio_util::io::ReaderStream;

use crate::pcm::PcmHub;

/// 出流 MP3 码率（kbps），与 ffmpeg `-b:a` 一致
pub const MP3_BITRATE_KBPS: u32 = 128;

/// Bridge 运行态
pub struct SpeakerCastBridge {
    /// 监听端口（0=未启动）
    port: AtomicU16,
    /// 局域网提示地址
    lan_hint: RwLock<Option<String>>,
    pcm: Arc<PcmHub>,
}

impl SpeakerCastBridge {
    /// 创建
    pub fn new(pcm: Arc<PcmHub>) -> Arc<Self> {
        Arc::new(Self {
            port: AtomicU16::new(0),
            lan_hint: RwLock::new(None),
            pcm,
        })
    }

    /// 当前端口
    pub fn port(&self) -> u16 {
        self.port.load(Ordering::Relaxed)
    }

    /// 启动本机 HTTP（幂等）
    pub async fn ensure_started(self: &Arc<Self>) -> crate::Result<u16> {
        let cur = self.port();
        if cur != 0 {
            return Ok(cur);
        }
        let bridge = Arc::clone(self);
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
        let port = listener.local_addr()?.port();
        self.port.store(port, Ordering::Relaxed);
        info!("SpeakerCast bridge listening 0.0.0.0:{port}");
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            .route("/cast/{did}/live", get(live_handler))
            .with_state(bridge);
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                warn!("SpeakerCast bridge serve error: {e}");
            }
        });
        Ok(port)
    }

    /// 构造给小爱的拉流 URL
    pub async fn stream_url_for(&self, did: &str, host: &str) -> String {
        let port = self.port();
        format!("http://{host}:{port}/cast/{did}/live")
    }

    /// 记录建议 host
    pub async fn set_lan_hint(&self, host: String) {
        *self.lan_hint.write().await = Some(host);
    }

    /// 读取 hint；若无则探测局域网 IPv4
    pub async fn lan_hint(&self) -> Option<String> {
        if let Some(h) = self.lan_hint.read().await.clone() {
            return Some(h);
        }
        detect_lan_ipv4()
    }

    /// 解析用于 play_url 的 host
    pub async fn resolve_host(&self) -> String {
        self.lan_hint()
            .await
            .unwrap_or_else(|| "127.0.0.1".into())
    }
}

/// 探测非回环 IPv4
pub fn detect_lan_ipv4() -> Option<String> {
    let ifaces = if_addrs::get_if_addrs().ok()?;
    ifaces
        .into_iter()
        .filter(|i| !i.is_loopback())
        .find_map(|i| match i.addr {
            if_addrs::IfAddr::V4(v) if !v.ip.is_link_local() => Some(v.ip.to_string()),
            _ => None,
        })
}

async fn live_handler(
    Path(did): Path<String>,
    State(bridge): State<Arc<SpeakerCastBridge>>,
) -> Response {
    let Some(pipe) = bridge.pcm.get(&did).await else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("no pcm for did"))
            .unwrap_or_else(|_| Response::new(Body::empty()));
    };
    let rate = pipe.rate();
    let ch = pipe.ch();
    let mut rx = pipe.subscribe();

    // 低延迟：小探测窗 + nobuffer + 128k；优先起播而非音质
    let mut child = match Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-fflags",
            "nobuffer",
            "-flags",
            "low_delay",
            "-probesize",
            "32",
            "-analyzeduration",
            "0",
            "-f",
            "f32le",
            "-ar",
            &rate.to_string(),
            "-ac",
            &ch.to_string(),
            "-i",
            "pipe:0",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "128k", // 与 MP3_BITRATE_KBPS 保持一致
            "-flush_packets",
            "1",
            "-f",
            "mp3",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("ffmpeg spawn failed: {e}");
            return Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Body::from(format!("ffmpeg: {e}")))
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }
    };

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");

    // 喂 PCM；无数据时写短静音保活
    tokio::spawn(async move {
        let silence = silence_f32(rate, ch, 20);
        let mut idle = 0u32;
        loop {
            match tokio::time::timeout(std::time::Duration::from_millis(40), rx.recv()).await {
                Ok(Ok(chunk)) => {
                    idle = 0;
                    if stdin.write_all(&chunk).await.is_err() {
                        break;
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(_)) => break,
                Err(_) => {
                    idle = idle.saturating_add(1);
                    // 前几秒允许等首包；之后补静音避免 ffmpeg 饿死
                    if idle > 5 && stdin.write_all(&silence).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = stdin.shutdown().await;
        let _ = child.wait().await;
    });

    let stream = ReaderStream::new(stdout).map(|r| r.map(Bytes::from));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "audio/mpeg")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONNECTION, "close")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

/// 生成约 ms 毫秒的交织 f32 静音
fn silence_f32(rate: u32, ch: u8, ms: u32) -> Vec<u8> {
    let n = (rate as u64 * ch as u64 * ms as u64 / 1000) as usize;
    vec![0u8; n * 4]
}
