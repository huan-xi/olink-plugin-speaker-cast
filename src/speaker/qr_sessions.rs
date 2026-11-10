//! 扫码登录内存会话（begin 后后台等 lp，poll 查状态）。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use log::{info, warn};

use super::auth::{qr_login_begin, qr_login_complete, MicoApiLogin};
use crate::{Result, SpeakerCastError};

const TTL: Duration = Duration::from_secs(180);

/// 扫码会话状态
#[derive(Debug, Clone)]
pub enum QrSessionStatus {
    /// 等待扫码
    Pending { login_url: String, qr: String },
    /// 成功
    Ok(MicoApiLogin),
    /// 失败
    Err(String),
}

struct Entry {
    status: QrSessionStatus,
    created: Instant,
}

/// 扫码会话表
#[derive(Clone, Default)]
pub struct QrSessionStore {
    inner: Arc<RwLock<HashMap<String, Entry>>>,
}

impl QrSessionStore {
    /// 创建
    pub fn new() -> Self {
        Self::default()
    }

    /// 发起扫码并后台等待完成；返回 (session_id, login_url, qr)
    pub async fn begin(&self) -> Result<(String, String, String)> {
        self.gc().await;
        let begin = qr_login_begin().await?;
        let session_id = uuid::Uuid::new_v4().to_string();
        let login_url = begin.login_url.clone();
        let qr = begin.qr.clone();
        {
            let mut g = self.inner.write().await;
            g.insert(
                session_id.clone(),
                Entry {
                    status: QrSessionStatus::Pending {
                        login_url: login_url.clone(),
                        qr: qr.clone(),
                    },
                    created: Instant::now(),
                },
            );
        }
        let store = self.clone();
        let sid = session_id.clone();
        tokio::spawn(async move {
            let result = qr_login_complete(&begin).await;
            let mut g = store.inner.write().await;
            if let Some(e) = g.get_mut(&sid) {
                e.status = match result {
                    Ok(login) => {
                        info!("speaker-cast qr login ok user_id={}", login.user_id);
                        QrSessionStatus::Ok(login)
                    }
                    Err(err) => {
                        warn!("speaker-cast qr login failed: {err}");
                        QrSessionStatus::Err(err.to_string())
                    }
                };
            }
        });
        Ok((session_id, login_url, qr))
    }

    /// 查询状态
    pub async fn poll(&self, session_id: &str) -> Result<QrSessionStatus> {
        let g = self.inner.read().await;
        let e = g
            .get(session_id)
            .ok_or_else(|| SpeakerCastError::Mina("扫码会话不存在或已过期".into()))?;
        if e.created.elapsed() > TTL {
            return Err(SpeakerCastError::Mina("扫码会话已过期".into()));
        }
        Ok(e.status.clone())
    }

    async fn gc(&self) {
        let mut g = self.inner.write().await;
        g.retain(|_, e| e.created.elapsed() <= TTL);
    }
}
