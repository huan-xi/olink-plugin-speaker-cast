//! 每路音箱 PCM 广播总线（AirPlay → Bridge）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use tokio::sync::broadcast;

/// 默认广播缓冲（块数）
const BUS_CAP: usize = 64;

/// 单路音频格式 + 广播
pub struct DidPcm {
    /// 采样率
    pub sample_rate: AtomicU32,
    /// 声道
    pub channels: AtomicU8,
    /// 最近一次推送的 epoch ms（0=从未）
    last_push_ms: AtomicU64,
    tx: broadcast::Sender<Bytes>,
}

impl DidPcm {
    fn new(sample_rate: u32, channels: u8) -> Self {
        let (tx, _) = broadcast::channel(BUS_CAP);
        Self {
            sample_rate: AtomicU32::new(sample_rate),
            channels: AtomicU8::new(channels.max(1)),
            last_push_ms: AtomicU64::new(0),
            tx,
        }
    }

    /// 订阅 PCM 块（f32 LE 字节）
    pub fn subscribe(&self) -> broadcast::Receiver<Bytes> {
        self.tx.subscribe()
    }

    /// 更新格式并推送交织 f32
    pub fn push_f32(&self, sample_rate: u32, channels: u8, samples: &[f32]) {
        if sample_rate > 0 {
            self.sample_rate.store(sample_rate, Ordering::Relaxed);
        }
        if channels > 0 {
            self.channels.store(channels.max(1), Ordering::Relaxed);
        }
        if samples.is_empty() {
            return;
        }
        let mut buf = Vec::with_capacity(samples.len() * 4);
        for s in samples {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        let _ = self.tx.send(Bytes::from(buf));
        self.last_push_ms.store(now_ms(), Ordering::Relaxed);
    }

    /// 当前采样率
    pub fn rate(&self) -> u32 {
        self.sample_rate.load(Ordering::Relaxed).max(1)
    }

    /// 当前声道
    pub fn ch(&self) -> u8 {
        self.channels.load(Ordering::Relaxed).max(1)
    }

    /// 最近 within_ms 内是否有 PCM 推送
    pub fn streaming(&self, within_ms: u64) -> bool {
        let last = self.last_push_ms.load(Ordering::Relaxed);
        if last == 0 {
            return false;
        }
        now_ms().saturating_sub(last) <= within_ms
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 多路 PCM 中枢
pub struct PcmHub {
    map: std::sync::RwLock<HashMap<String, Arc<DidPcm>>>,
}

impl PcmHub {
    /// 创建
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            map: std::sync::RwLock::new(HashMap::new()),
        })
    }

    /// 确保某 did 通道存在（同步，供 audio 热路径）
    pub fn ensure_sync(&self, did: &str, sample_rate: u32, channels: u8) -> Arc<DidPcm> {
        if let Ok(g) = self.map.read() {
            if let Some(p) = g.get(did) {
                if sample_rate > 0 {
                    p.sample_rate.store(sample_rate, Ordering::Relaxed);
                }
                if channels > 0 {
                    p.channels.store(channels.max(1), Ordering::Relaxed);
                }
                return Arc::clone(p);
            }
        }
        let mut g = self.map.write().expect("pcm hub");
        if let Some(p) = g.get(did) {
            return Arc::clone(p);
        }
        let p = Arc::new(DidPcm::new(sample_rate.max(1), channels.max(1)));
        g.insert(did.to_string(), Arc::clone(&p));
        p
    }

    /// 异步确保
    pub async fn ensure(&self, did: &str, sample_rate: u32, channels: u8) -> Arc<DidPcm> {
        self.ensure_sync(did, sample_rate, channels)
    }

    /// 取通道
    pub async fn get(&self, did: &str) -> Option<Arc<DidPcm>> {
        self.map.read().ok()?.get(did).cloned()
    }

    /// 热路径推送（无 await）
    pub fn push_f32_sync(&self, did: &str, sample_rate: u32, channels: u8, samples: &[f32]) {
        self.ensure_sync(did, sample_rate, channels)
            .push_f32(sample_rate, channels, samples);
    }

    /// 推送
    pub async fn push_f32(&self, did: &str, sample_rate: u32, channels: u8, samples: &[f32]) {
        self.push_f32_sync(did, sample_rate, channels, samples);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_subscribe() {
        let hub = PcmHub::new();
        let p = hub.ensure_sync("did1", 44100, 2);
        let mut rx = p.subscribe();
        hub.push_f32_sync("did1", 44100, 2, &[0.0, 0.5, -0.5, 1.0]);
        let chunk = rx.try_recv().expect("pcm");
        assert_eq!(chunk.len(), 16);
        assert_eq!(p.rate(), 44100);
        assert_eq!(p.ch(), 2);
        assert!(p.streaming(5_000));
    }
}
