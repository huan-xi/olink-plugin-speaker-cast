//! 多账号 micoapi 凭据落盘（`accounts.json`；兼容旧 `mina.json`）。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::auth::MicoApiLogin;
use crate::{Result, SpeakerCastError};

/// 单条小米账号（micoapi）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MinaAccount {
    /// 小米 userId（主键）
    pub user_id: String,
    /// micoapi serviceToken
    pub service_token: String,
    /// Passport passToken
    #[serde(default)]
    pub pass_token: String,
    /// ssecurity（可选）
    #[serde(default)]
    pub ssecurity: String,
    /// 展示名
    #[serde(default)]
    pub nickname: String,
    /// Token 到期（毫秒）
    #[serde(default)]
    pub expire_at_ms: u64,
    /// 创建时间（毫秒）
    #[serde(default)]
    pub created_at_ms: u64,
}

impl MinaAccount {
    /// 是否可用
    pub fn is_ready(&self) -> bool {
        !self.user_id.is_empty() && !self.service_token.is_empty()
    }

    /// 从登录结果构造
    pub fn from_login(login: &MicoApiLogin, nickname: &str) -> Self {
        let now = now_ms();
        Self {
            user_id: login.user_id.clone(),
            service_token: login.service_token.clone(),
            pass_token: login.pass_token.clone(),
            ssecurity: login.ssecurity.clone(),
            nickname: if nickname.is_empty() {
                login.user_id.clone()
            } else {
                nickname.to_string()
            },
            expire_at_ms: if login.expire_at_ms > 0 {
                login.expire_at_ms
            } else {
                now + 30 * 24 * 3600 * 1000
            },
            created_at_ms: now,
        }
    }
}

/// 列表对外脱敏视图
#[derive(Debug, Clone, Serialize)]
pub struct MinaAccountView {
    pub user_id: String,
    pub nickname: String,
    /// RFC3339；无则 null
    pub expire_at: Option<String>,
    pub ready: bool,
}

impl From<&MinaAccount> for MinaAccountView {
    fn from(a: &MinaAccount) -> Self {
        Self {
            user_id: a.user_id.clone(),
            nickname: if a.nickname.is_empty() {
                a.user_id.clone()
            } else {
                a.nickname.clone()
            },
            expire_at: ms_to_rfc3339(a.expire_at_ms),
            ready: a.is_ready(),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AccountsFile {
    accounts: Vec<MinaAccount>,
}

/// 兼容旧单票
#[derive(Debug, Deserialize)]
struct LegacyMina {
    user_id: String,
    service_token: String,
    #[serde(default)]
    pass_token: String,
}

/// 读写 `{data}/speaker-cast/accounts.json`
pub struct MinaCredStore {
    dir: PathBuf,
    path: PathBuf,
    legacy: PathBuf,
}

impl MinaCredStore {
    /// 打开存储
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        let dir = data_dir.as_ref().join("speaker-cast");
        Self {
            path: dir.join("accounts.json"),
            legacy: dir.join("mina.json"),
            dir,
        }
    }

    /// 加载全部账号（自动迁移旧 mina.json）
    pub async fn load_all(&self) -> Result<Vec<MinaAccount>> {
        tokio::fs::create_dir_all(&self.dir).await?;
        if self.path.exists() {
            let raw = tokio::fs::read_to_string(&self.path).await?;
            let file: AccountsFile = serde_json::from_str(&raw)
                .map_err(|e| SpeakerCastError::Mina(format!("accounts json: {e}")))?;
            return Ok(file.accounts);
        }
        if self.legacy.exists() {
            let raw = tokio::fs::read_to_string(&self.legacy).await?;
            if let Ok(legacy) = serde_json::from_str::<LegacyMina>(&raw) {
                let acc = MinaAccount {
                    user_id: legacy.user_id.clone(),
                    service_token: legacy.service_token,
                    pass_token: legacy.pass_token,
                    nickname: legacy.user_id.clone(),
                    expire_at_ms: now_ms() + 30 * 24 * 3600 * 1000,
                    created_at_ms: now_ms(),
                    ..Default::default()
                };
                let list = vec![acc];
                self.save_all(&list).await?;
                let bak = self.dir.join("mina.json.bak");
                let _ = tokio::fs::rename(&self.legacy, &bak).await;
                return Ok(list);
            }
        }
        Ok(vec![])
    }

    /// 全量保存
    pub async fn save_all(&self, accounts: &[MinaAccount]) -> Result<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        let file = AccountsFile {
            accounts: accounts.to_vec(),
        };
        let raw = serde_json::to_string_pretty(&file)
            .map_err(|e| SpeakerCastError::Mina(format!("accounts ser: {e}")))?;
        tokio::fs::write(&self.path, raw).await?;
        Ok(())
    }

    /// 按 user_id 取
    pub async fn get(&self, user_id: &str) -> Result<Option<MinaAccount>> {
        Ok(self
            .load_all()
            .await?
            .into_iter()
            .find(|a| a.user_id == user_id))
    }

    /// 插入或更新
    pub async fn upsert(&self, account: MinaAccount) -> Result<MinaAccount> {
        let mut list = self.load_all().await?;
        if let Some(i) = list.iter().position(|a| a.user_id == account.user_id) {
            let mut merged = account;
            if merged.created_at_ms == 0 {
                merged.created_at_ms = list[i].created_at_ms;
            }
            if merged.nickname.is_empty() {
                merged.nickname = list[i].nickname.clone();
            }
            list[i] = merged.clone();
            self.save_all(&list).await?;
            Ok(merged)
        } else {
            list.push(account.clone());
            self.save_all(&list).await?;
            Ok(account)
        }
    }

    /// 删除
    pub async fn remove(&self, user_id: &str) -> Result<bool> {
        let mut list = self.load_all().await?;
        let before = list.len();
        list.retain(|a| a.user_id != user_id);
        if list.len() == before {
            return Ok(false);
        }
        self.save_all(&list).await?;
        Ok(true)
    }
}

/// 兼容旧名：单票视图（取第一账号）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MinaCreds {
    pub user_id: String,
    pub service_token: String,
    #[serde(default)]
    pub pass_token: String,
}

impl MinaCreds {
    pub fn is_ready(&self) -> bool {
        !self.user_id.is_empty() && !self.service_token.is_empty()
    }
}

impl From<&MinaAccount> for MinaCreds {
    fn from(a: &MinaAccount) -> Self {
        Self {
            user_id: a.user_id.clone(),
            service_token: a.service_token.clone(),
            pass_token: a.pass_token.clone(),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn ms_to_rfc3339(ms: u64) -> Option<String> {
    if ms == 0 {
        return None;
    }
    let secs = (ms / 1000) as i64;
    chrono::DateTime::from_timestamp(secs, 0).map(|dt| dt.to_rfc3339())
}
