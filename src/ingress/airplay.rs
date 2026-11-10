//! AirPlay / RAOP 接收（shairplay）。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

use log::{info, warn};
use serde::Serialize;
use shairplay::{
    Ap1Codec, Ap1Encryption, AudioFormat, AudioHandler, AudioSession, RaopServer,
};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;

use crate::access::SpeakerAccess;
use crate::pcm::PcmHub;

/// AirPlay → 业务层事件
#[derive(Debug, Clone)]
pub enum AirPlayEvent {
    /// 开流，应 play_url
    SessionStart { did: String },
    /// 客户端调音量（dB：0=最大，-144=静音）
    Volume { did: String, db: f32 },
}

/// 单路接收状态
#[derive(Debug, Clone, Serialize)]
pub struct AirPlayReceiverStatus {
    pub did: String,
    pub airplay_name: String,
    pub running: bool,
    pub raop_port: u16,
    pub detail: String,
}

struct Running {
    status: AirPlayReceiverStatus,
    stop: tokio::sync::oneshot::Sender<()>,
    join: JoinHandle<()>,
}

/// AirPlay Ingress
pub struct AirPlayIngress {
    pcm: Arc<PcmHub>,
    /// 会话 / 音量等事件（任意线程可 send）
    event_tx: mpsc::UnboundedSender<AirPlayEvent>,
    running: RwLock<HashMap<String, Running>>,
    /// 下一候选 RAOP 端口（auto_port 会自增）
    next_port: AtomicU16,
}

impl AirPlayIngress {
    /// 创建；返回 (ingress, 事件 rx)
    pub fn new(pcm: Arc<PcmHub>) -> (Arc<Self>, mpsc::UnboundedReceiver<AirPlayEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Arc::new(Self {
                pcm,
                event_tx: tx,
                running: RwLock::new(HashMap::new()),
                next_port: AtomicU16::new(5100),
            }),
            rx,
        )
    }

    /// 启动某音箱对应的接收端
    pub async fn start(self: &Arc<Self>, speaker: &SpeakerAccess) {
        self.stop(&speaker.did).await;
        let port = self.next_port.fetch_add(10, Ordering::Relaxed);
        let did = speaker.did.clone();
        let name = speaker.airplay_name.clone();
        let hw = hwaddr_from_did(&did);
        let pcm = Arc::clone(&self.pcm);
        let event_tx = self.event_tx.clone();
        let handler: Arc<dyn AudioHandler> = Arc::new(CastHandler {
            did: did.clone(),
            pcm,
            event_tx,
        });

        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let did_task = did.clone();
        let name_task = name.clone();
        let join = tokio::spawn(async move {
            let mut server = match RaopServer::builder()
                .name(name_task.clone())
                .hwaddr(hw.to_vec())
                .port(port)
                .output_sample_rate(44100)
                .output_max_channels(2)
                .advertise_codecs(vec![Ap1Codec::Pcm, Ap1Codec::Alac])
                .advertise_encryption(vec![Ap1Encryption::Rsa, Ap1Encryption::FairPlay])
                .build(handler)
            {
                Ok(s) => s,
                Err(e) => {
                    warn!("RaopServer build failed did={did_task}: {e}");
                    return;
                }
            };
            if let Err(e) = server.start().await {
                warn!("RaopServer start failed did={did_task}: {e}");
                return;
            }
            info!("AirPlayIngress RAOP up did={did_task} name={name_task} port~={port}");
            let _ = stop_rx.await;
            server.stop().await;
            info!("AirPlayIngress RAOP stopped did={did_task}");
        });

        let st = AirPlayReceiverStatus {
            did: did.clone(),
            airplay_name: name,
            running: true,
            raop_port: port,
            detail: "shairplay RAOP running".into(),
        };
        self.running.write().await.insert(
            did,
            Running {
                status: st,
                stop: stop_tx,
                join,
            },
        );
    }

    /// 停止
    pub async fn stop(&self, did: &str) {
        let Some(r) = self.running.write().await.remove(did) else {
            return;
        };
        let _ = r.stop.send(());
        let _ = r.join.await;
        info!("AirPlayIngress stop did={did}");
    }

    /// 全部状态
    pub async fn list_status(&self) -> Vec<AirPlayReceiverStatus> {
        self.running
            .read()
            .await
            .values()
            .map(|r| r.status.clone())
            .collect()
    }
}

struct CastHandler {
    did: String,
    pcm: Arc<PcmHub>,
    event_tx: mpsc::UnboundedSender<AirPlayEvent>,
}

impl AudioHandler for CastHandler {
    fn audio_init(&self, format: AudioFormat) -> Box<dyn AudioSession> {
        let rate = format.sample_rate.max(1);
        let ch = format.channels.max(1);
        self.pcm.ensure_sync(&self.did, rate, ch);
        let _ = self.event_tx.send(AirPlayEvent::SessionStart {
            did: self.did.clone(),
        });
        Box::new(CastSession {
            did: self.did.clone(),
            pcm: Arc::clone(&self.pcm),
            sample_rate: rate,
            channels: ch,
        })
    }

    fn on_volume(&self, volume: f32) {
        let _ = self.event_tx.send(AirPlayEvent::Volume {
            did: self.did.clone(),
            db: volume,
        });
    }

    fn on_client_connected(&self, addr: &str) {
        info!("AirPlay client connected did={} addr={addr}", self.did);
    }

    fn on_client_disconnected(&self, addr: &str) {
        info!("AirPlay client disconnected did={} addr={addr}", self.did);
    }

    fn on_error(&self, error: &shairplay::ShairplayError) {
        warn!("AirPlay error did={}: {error}", self.did);
    }
}

struct CastSession {
    did: String,
    pcm: Arc<PcmHub>,
    sample_rate: u32,
    channels: u8,
}

impl AudioSession for CastSession {
    fn audio_process(&mut self, samples: &[f32]) {
        self.pcm
            .push_f32_sync(&self.did, self.sample_rate, self.channels, samples);
    }

    fn audio_flush(&mut self) {}
}

/// 由 did 派生稳定本地管理 MAC
fn hwaddr_from_did(did: &str) -> [u8; 6] {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    did.hash(&mut h);
    let v = h.finish().to_le_bytes();
    [0x02, v[0], v[1], v[2], v[3], v[4]]
}
