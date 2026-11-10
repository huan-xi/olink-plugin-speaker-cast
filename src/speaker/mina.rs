//! MiNA HTTP：api2.mina.mi.com + ubus 播控。
//!
//! 协议对齐 MiService / miair-next：`play_by_url` / `play_by_music_url`。

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::{Result, SpeakerCastError};

/// 需走 `player_play_music` 的硬件型号（触屏封面等）
const USE_PLAY_MUSIC_API: &[&str] = &[
    "LX04", "LX05", "L05B", "L05C", "L06", "L06A", "X08A", "X10A", "X08C", "X08E", "X8F", "X4B",
    "OH2", "OH2P", "X6A",
];

/// 默认 audioID（无曲库命中时占位，同生态常见默认）
const DEFAULT_AUDIO_ID: &str = "1582971365183456177";

/// 型号是否必须走 player_play_music
fn hardware_needs_music_api(hardware: &str) -> bool {
    USE_PLAY_MUSIC_API
        .iter()
        .any(|x| x.eq_ignore_ascii_case(hardware))
}

/// MiNA 音箱条目
#[derive(Debug, Clone)]
pub struct MinaDevice {
    /// MiNA deviceId（ubus 用，不是 MIoT did）
    pub device_id: String,
    /// 展示名
    pub name: String,
    /// 硬件型号，如 LX06
    pub hardware: String,
    /// 关联的 MIoT did（若列表带有）
    pub miot_did: Option<String>,
}

/// MiNA 客户端（持有 micoapi serviceToken）
pub struct MinaClient {
    http: reqwest::Client,
    /// userId
    user_id: String,
    /// micoapi serviceToken
    service_token: String,
    /// deviceId → hardware
    hardware_by_device: HashMap<String, String>,
}

impl MinaClient {
    /// 用已换好的 micoapi 凭据构造（登录态由上层负责）
    pub fn from_tokens(user_id: impl Into<String>, service_token: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            user_id: user_id.into(),
            service_token: service_token.into(),
            hardware_by_device: HashMap::new(),
        }
    }

    /// 是否应使用 play_music API
    fn use_music_api(&self, device_id: &str) -> bool {
        self.hardware_by_device
            .get(device_id)
            .map(|h| hardware_needs_music_api(h))
            .unwrap_or(true)
    }

    /// 拉取音箱列表并缓存 hardware（GET；POST 会 601 MethodNotSupported）
    pub async fn device_list(&mut self) -> Result<Vec<MinaDevice>> {
        let raw = self
            .mina_get("/admin/v2/device_list?master=0")
            .await?;
        let data = raw
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut out = Vec::new();
        self.hardware_by_device.clear();
        for item in data {
            let device_id = item
                .get("deviceID")
                .or_else(|| item.get("deviceId"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if device_id.is_empty() {
                continue;
            }
            let hardware = item
                .get("hardware")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            self.hardware_by_device
                .insert(device_id.clone(), hardware.clone());
            out.push(MinaDevice {
                device_id,
                name: item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                hardware,
                miot_did: item
                    .get("miotDID")
                    .or_else(|| item.get("miotDid"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            });
        }
        Ok(out)
    }


    /// 播放 URL（按型号自动选 API）
    pub async fn play_url(&self, device_id: &str, url: &str) -> Result<()> {
        if self.use_music_api(device_id) {
            self.play_by_music_url(device_id, url, DEFAULT_AUDIO_ID)
                .await
        } else {
            self.ubus(
                device_id,
                "player_play_url",
                "mediaplayer",
                json!({ "url": url, "type": 2, "media": "app_ios" }),
            )
            .await
        }
    }

    /// 触屏机常用 play_music
    pub async fn play_by_music_url(&self, device_id: &str, url: &str, audio_id: &str) -> Result<()> {
        let music = json!({
            "payload": {
                "audio_type": "",
                "audio_items": [{
                    "item_id": {
                        "audio_id": audio_id,
                        "cp": { "album_id": "-1", "id": "355454500", "name": "xiaowei" }
                    },
                    "stream": { "url": url }
                }],
                "play_behavior": 1
            }
        });
        self.ubus(
            device_id,
            "player_play_music",
            "mediaplayer",
            json!({ "music": serde_json::to_string(&music).unwrap_or_default(), "media": "app_ios" }),
        )
        .await
    }

    /// 暂停
    pub async fn pause(&self, device_id: &str) -> Result<()> {
        if self.use_music_api(device_id) {
            self.stop(device_id).await
        } else {
            self.ubus(
                device_id,
                "player_play_operation",
                "mediaplayer",
                json!({ "action": "pause", "media": "app_ios" }),
            )
            .await
        }
    }

    /// 停止（连发 stop/pause，减轻缓冲尾音）
    pub async fn stop(&self, device_id: &str) -> Result<()> {
        let _ = self
            .ubus(
                device_id,
                "player_play_operation",
                "mediaplayer",
                json!({ "action": "stop", "media": "app_ios" }),
            )
            .await;
        let _ = self
            .ubus(
                device_id,
                "player_play_operation",
                "mediaplayer",
                json!({ "action": "pause", "media": "app_ios" }),
            )
            .await;
        self.ubus(
            device_id,
            "player_play_operation",
            "mediaplayer",
            json!({ "action": "stop", "media": "common" }),
        )
        .await
    }

    /// 音量 0–100
    pub async fn set_volume(&self, device_id: &str, volume: u8) -> Result<()> {
        let volume = volume.min(100);
        self.ubus(
            device_id,
            "player_set_volume",
            "mediaplayer",
            json!({ "volume": volume, "media": "app_ios" }),
        )
        .await
    }

    async fn ubus(&self, device_id: &str, method: &str, path: &str, message: Value) -> Result<()> {
        let body = json!({
            "deviceId": device_id,
            "message": message.to_string(),
            "method": method,
            "path": path,
        });
        let raw = self.mina_request("/remote/ubus", Some(body)).await?;
        let code = raw.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code == 0 {
            Ok(())
        } else {
            Err(SpeakerCastError::Mina(format!(
                "ubus {method} code={code} resp={raw}"
            )))
        }
    }

    async fn mina_get(&self, uri: &str) -> Result<Value> {
        self.mina_http(reqwest::Method::GET, uri, None).await
    }

    async fn mina_request(&self, uri: &str, data: Option<Value>) -> Result<Value> {
        self.mina_http(reqwest::Method::POST, uri, data).await
    }

    async fn mina_http(
        &self,
        method: reqwest::Method,
        uri: &str,
        data: Option<Value>,
    ) -> Result<Value> {
        // Cookie 形态对齐生态：userId + serviceToken；完整签名登录在 auth 模块补齐。
        let url = format!("https://api2.mina.mi.com{uri}");
        let mut req = self
            .http
            .request(method, &url)
            .header(
                "Cookie",
                format!(
                    "userId={}; serviceToken={}",
                    self.user_id, self.service_token
                ),
            )
            .header("User-Agent", "MiSoundBox/2.0");
        if let Some(d) = data {
            req = req.form(&flatten_form(&d));
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(SpeakerCastError::Mina(format!(
                "http {status}: {}",
                truncate(&text, 200)
            )));
        }
        serde_json::from_str(&text).map_err(|e| {
            SpeakerCastError::Mina(format!("json: {e}; body={}", truncate(&text, 200)))
        })
    }
}

fn flatten_form(v: &Value) -> Vec<(String, String)> {
    match v.as_object() {
        Some(map) => map
            .iter()
            .map(|(k, val)| {
                (
                    k.clone(),
                    match val {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    },
                )
            })
            .collect(),
        None => vec![],
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// 在设备列表中按 miot did 匹配
pub fn find_device_by_miot_did<'a>(
    devices: &'a [MinaDevice],
    miot_did: &str,
) -> Option<&'a MinaDevice> {
    let want = miot_did.trim();
    devices.iter().find(|d| {
        d.miot_did.as_deref() == Some(want)
            || d.miot_did.as_deref().is_some_and(|x| x.ends_with(want))
            || d.device_id == want
    })
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn music_api_whitelist_case_insensitive() {
        assert!(!hardware_needs_music_api("lx06"));
        assert!(hardware_needs_music_api("LX04"));
        assert!(hardware_needs_music_api("oh2p"));
    }

    #[test]
    fn find_by_miot_did() {
        let list = vec![MinaDevice {
            device_id: "a".into(),
            name: "n".into(),
            hardware: "LX06".into(),
            miot_did: Some("123456".into()),
        }];
        assert_eq!(find_device_by_miot_did(&list, "123456").unwrap().device_id, "a");
    }
}
