//! 统一投送会话模型（协议无关）。

use std::time::Duration;

use async_trait::async_trait;

use crate::Result;

/// 投送会话 ID
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MediaSessionId(pub String);

/// 曲目元数据（封面 / 歌词匹配等）
#[derive(Debug, Clone, Default)]
pub struct TrackMeta {
    /// 歌名
    pub title: Option<String>,
    /// 歌手
    pub artist: Option<String>,
    /// 专辑
    pub album: Option<String>,
}

/// 音频载荷：本期以 PCM 流为主；URL 为后续协议预留
#[derive(Debug, Clone)]
pub enum MediaPayload {
    /// 实时 PCM（AirPlay 解密后）
    PcmStream {
        /// 采样率
        sample_rate: u32,
        /// 声道数
        channels: u16,
    },
    /// 可播 URL（DLNA 等预留，本期 Bridge 可拒绝）
    UrlSource {
        /// 媒体地址
        url: String,
        /// Content-Type（可选）
        content_type: Option<String>,
    },
}

/// PCM 一帧
#[derive(Debug, Clone)]
pub struct PcmFrame {
    /// 交织 PCM（通常 i16 LE）
    pub samples: Vec<i16>,
}

/// 会话事件汇入出声桥
#[async_trait]
pub trait MediaSessionSink: Send + Sync {
    /// 开始播放
    async fn on_play(&self, session: MediaSessionId, payload: MediaPayload) -> Result<()>;
    /// 暂停
    async fn on_pause(&self, session: MediaSessionId) -> Result<()>;
    /// 继续
    async fn on_resume(&self, session: MediaSessionId) -> Result<()>;
    /// 跳转
    async fn on_seek(&self, session: MediaSessionId, position: Duration) -> Result<()>;
    /// 停止
    async fn on_stop(&self, session: MediaSessionId) -> Result<()>;
    /// 音量 0.0–1.0
    async fn on_volume(&self, session: MediaSessionId, volume: f32) -> Result<()>;
    /// 元数据
    async fn on_metadata(&self, session: MediaSessionId, meta: TrackMeta) -> Result<()>;
    /// 持续喂 PCM
    async fn on_pcm(&self, session: MediaSessionId, frame: PcmFrame) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct RecSink {
        plays: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl MediaSessionSink for RecSink {
        async fn on_play(&self, session: MediaSessionId, _payload: MediaPayload) -> Result<()> {
            self.plays.lock().unwrap().push(session.0);
            Ok(())
        }
        async fn on_pause(&self, _: MediaSessionId) -> Result<()> {
            Ok(())
        }
        async fn on_resume(&self, _: MediaSessionId) -> Result<()> {
            Ok(())
        }
        async fn on_seek(&self, _: MediaSessionId, _: Duration) -> Result<()> {
            Ok(())
        }
        async fn on_stop(&self, _: MediaSessionId) -> Result<()> {
            Ok(())
        }
        async fn on_volume(&self, _: MediaSessionId, _: f32) -> Result<()> {
            Ok(())
        }
        async fn on_metadata(&self, _: MediaSessionId, _: TrackMeta) -> Result<()> {
            Ok(())
        }
        async fn on_pcm(&self, _: MediaSessionId, _: PcmFrame) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn sink_records_play() {
        let sink = RecSink {
            plays: Mutex::new(vec![]),
        };
        sink.on_play(
            MediaSessionId("s1".into()),
            MediaPayload::PcmStream {
                sample_rate: 44100,
                channels: 2,
            },
        )
        .await
        .unwrap();
        assert_eq!(sink.plays.lock().unwrap().as_slice(), ["s1"]);
    }
}
