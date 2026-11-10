//! 面向 Bridge 的音箱控制门面。

use std::sync::Arc;

use tokio::sync::Mutex;

use super::mina::MinaClient;
use crate::Result;

/// 绑定一台目标音箱的控制句柄
pub struct SpeakerControl {
    mina: Arc<Mutex<MinaClient>>,
    /// 当前 MiNA deviceId
    device_id: String,
}

impl SpeakerControl {
    /// 创建
    pub fn new(mina: MinaClient, device_id: impl Into<String>) -> Self {
        Self {
            mina: Arc::new(Mutex::new(mina)),
            device_id: device_id.into(),
        }
    }

    /// 播放可访问 URL
    pub async fn play_url(&self, url: &str) -> Result<()> {
        let g = self.mina.lock().await;
        g.play_url(&self.device_id, url).await
    }

    /// 停止
    pub async fn stop(&self) -> Result<()> {
        let g = self.mina.lock().await;
        g.stop(&self.device_id).await
    }

    /// 暂停
    pub async fn pause(&self) -> Result<()> {
        let g = self.mina.lock().await;
        g.pause(&self.device_id).await
    }

    /// 音量 0–100
    pub async fn set_volume(&self, volume: u8) -> Result<()> {
        let g = self.mina.lock().await;
        g.set_volume(&self.device_id, volume).await
    }
}
