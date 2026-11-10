//! 音箱投送 API 请求。

use serde::Deserialize;

/// 音响接入
#[derive(Debug, Deserialize)]
pub struct ConnectSpeakerReq {
    /// 米家 did（或 miotDID）
    pub did: String,
    /// 展示名
    pub name: Option<String>,
    /// MiNA deviceId；省略则按 miotDID 自动解析
    pub mina_device_id: Option<String>,
    /// AirPlay 广播名
    pub airplay_name: Option<String>,
    /// 小米账号 userId
    pub account_id: String,
}

/// 按 did
#[derive(Debug, Deserialize)]
pub struct DidReq {
    pub did: String,
}

/// MiNA 凭据（手填 upsert）
#[derive(Debug, Deserialize)]
pub struct MinaTokenReq {
    pub user_id: String,
    pub service_token: String,
    #[serde(default)]
    pub pass_token: Option<String>,
}

/// 扫码 poll
#[derive(Debug, Deserialize)]
pub struct MinaQrPollReq {
    pub session_id: String,
}

/// 试播
#[derive(Debug, Deserialize)]
pub struct TestPlayReq {
    pub did: String,
    /// 可被小爱访问的 http(s) 音频 URL
    pub url: String,
}
