//! 音箱投送 API 响应。

use serde::Serialize;

use crate::access::SpeakerAccess;
use crate::ingress::AirPlayReceiverStatus;
use crate::speaker::{MinaAccountView, MinaDevice};

/// 接入条目（含运行态）
#[derive(Debug, Serialize)]
pub struct SpeakerAccessResp {
    pub did: String,
    pub name: String,
    pub mina_device_id: String,
    pub airplay_enabled: bool,
    pub airplay_name: String,
    pub account_id: String,
    /// Bridge HTTP 端口；0=未起
    pub bridge_port: u16,
    /// RAOP 接收端是否在跑
    pub raop_running: bool,
    /// RAOP 端口
    pub raop_port: u16,
    /// 出流 MP3 码率 kbps
    pub bitrate_kbps: u32,
    /// PCM 采样率
    pub sample_rate: u32,
    /// 声道数
    pub channels: u8,
    /// 近几秒是否有 PCM（推流中）
    pub streaming: bool,
}

impl SpeakerAccessResp {
    /// 从接入记录 + 运行态组装
    pub fn from_access(
        s: SpeakerAccess,
        bridge_port: u16,
        raop_running: bool,
        raop_port: u16,
        bitrate_kbps: u32,
        sample_rate: u32,
        channels: u8,
        streaming: bool,
    ) -> Self {
        Self {
            did: s.did,
            name: s.name,
            mina_device_id: s.mina_device_id,
            airplay_enabled: s.airplay_enabled,
            airplay_name: s.airplay_name,
            account_id: s.account_id,
            bridge_port,
            raop_running,
            raop_port,
            bitrate_kbps,
            sample_rate,
            channels,
            streaming,
        }
    }
}

/// MiNA 设备
#[derive(Debug, Serialize)]
pub struct MinaDeviceResp {
    pub device_id: String,
    pub name: String,
    pub hardware: String,
    pub miot_did: Option<String>,
}

impl From<MinaDevice> for MinaDeviceResp {
    fn from(d: MinaDevice) -> Self {
        Self {
            device_id: d.device_id,
            name: d.name,
            hardware: d.hardware,
            miot_did: d.miot_did,
        }
    }
}

/// 总览状态
#[derive(Debug, Serialize)]
pub struct CastStatusResp {
    pub mina_ready: bool,
    pub accounts_count: usize,
    pub bridge_port: u16,
    pub receivers: Vec<AirPlayReceiverStatus>,
}

/// 扫码 begin
#[derive(Debug, Serialize)]
pub struct MinaQrBeginResp {
    pub session_id: String,
    pub login_url: String,
    pub qr: String,
}

/// 扫码 poll
#[derive(Debug, Serialize)]
pub struct MinaQrPollResp {
    /// pending | ok | error
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

/// 账号列表项
pub type AccountResp = MinaAccountView;
