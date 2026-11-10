//! 音响接入与 AirPlay 启停状态（JSON 持久化）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{Result, SpeakerCastError};

/// 一台已接入投送的音箱
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerAccess {
    /// 米家 did（无 domain 前缀的原始 did）
    pub did: String,
    /// 展示名
    pub name: String,
    /// MiNA deviceId（ubus 播控）
    pub mina_device_id: String,
    /// 是否已开启 AirPlay 接收
    pub airplay_enabled: bool,
    /// 局域网 AirPlay 显示名
    pub airplay_name: String,
    /// 所属小米账号 userId
    #[serde(default)]
    pub account_id: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AccessFile {
    speakers: HashMap<String, SpeakerAccess>,
}

/// 接入登记表
#[derive(Clone)]
pub struct AccessRegistry {
    path: PathBuf,
    inner: Arc<RwLock<AccessFile>>,
}

impl AccessRegistry {
    /// 从数据目录加载（文件不存在则空表）
    pub async fn load(data_dir: impl AsRef<Path>) -> Result<Self> {
        let dir = data_dir.as_ref().join("speaker-cast");
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join("access.json");
        let file = if path.exists() {
            let raw = tokio::fs::read_to_string(&path).await?;
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            AccessFile::default()
        };
        Ok(Self {
            path,
            inner: Arc::new(RwLock::new(file)),
        })
    }

    /// 列表
    pub async fn list(&self) -> Vec<SpeakerAccess> {
        self.inner.read().await.speakers.values().cloned().collect()
    }

    /// 按 did 取
    pub async fn get(&self, did: &str) -> Option<SpeakerAccess> {
        self.inner.read().await.speakers.get(did).cloned()
    }

    /// 音响接入（登记）；已存在则更新映射，保留 airplay_enabled
    pub async fn connect(
        &self,
        did: &str,
        name: &str,
        mina_device_id: &str,
        airplay_name: Option<String>,
        account_id: &str,
    ) -> Result<SpeakerAccess> {
        if did.is_empty() {
            return Err(SpeakerCastError::Session("did 不能为空".into()));
        }
        if account_id.is_empty() {
            return Err(SpeakerCastError::Session("account_id 不能为空".into()));
        }
        // mina_device_id 可空：先登记音箱，稍后配 MiNA 再 play_url
        let mut g = self.inner.write().await;
        let airplay_enabled = g
            .speakers
            .get(did)
            .map(|s| s.airplay_enabled)
            .unwrap_or(false);
        let airplay_name = airplay_name
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if name.is_empty() {
                    format!("Olink {did}")
                } else {
                    name.to_string()
                }
            });
        let row = SpeakerAccess {
            did: did.to_string(),
            name: if name.is_empty() {
                did.to_string()
            } else {
                name.to_string()
            },
            mina_device_id: mina_device_id.to_string(),
            airplay_enabled,
            airplay_name,
            account_id: account_id.to_string(),
        };
        g.speakers.insert(did.to_string(), row.clone());
        self.persist_locked(&g).await?;
        Ok(row)
    }

    /// 删除某账号下全部接入
    pub async fn disconnect_by_account(&self, account_id: &str) -> Result<Vec<String>> {
        let mut g = self.inner.write().await;
        let remove: Vec<String> = g
            .speakers
            .iter()
            .filter(|(_, s)| s.account_id == account_id)
            .map(|(k, _)| k.clone())
            .collect();
        for k in &remove {
            g.speakers.remove(k);
        }
        self.persist_locked(&g).await?;
        Ok(remove)
    }

    /// 取消接入
    pub async fn disconnect(&self, did: &str) -> Result<()> {
        let mut g = self.inner.write().await;
        g.speakers.remove(did);
        self.persist_locked(&g).await
    }

    /// 开/关 AirPlay（须已接入）
    pub async fn set_airplay(&self, did: &str, enabled: bool) -> Result<SpeakerAccess> {
        let mut g = self.inner.write().await;
        let row = g
            .speakers
            .get_mut(did)
            .ok_or_else(|| SpeakerCastError::Session(format!("未接入: {did}")))?;
        row.airplay_enabled = enabled;
        let out = row.clone();
        self.persist_locked(&g).await?;
        Ok(out)
    }

    async fn persist_locked(&self, g: &AccessFile) -> Result<()> {
        let raw = serde_json::to_string_pretty(g)
            .map_err(|e| SpeakerCastError::Bridge(format!("serialize: {e}")))?;
        tokio::fs::write(&self.path, raw).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_enable_airplay_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let reg = AccessRegistry::load(dir.path()).await.unwrap();
        let s = reg
            .connect("123", "客厅", "mina-1", Some("客厅小爱".into()), "uid1")
            .await
            .unwrap();
        assert!(!s.airplay_enabled);
        let s = reg.set_airplay("123", true).await.unwrap();
        assert!(s.airplay_enabled);
        let reg2 = AccessRegistry::load(dir.path()).await.unwrap();
        assert!(reg2.get("123").await.unwrap().airplay_enabled);
    }
}
