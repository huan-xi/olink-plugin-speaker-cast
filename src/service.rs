//! 业务编排：多账号 / 接入 / 开 AirPlay / 真收流后 play_url。

use std::sync::Arc;

use log::{info, warn};
use tokio::sync::{mpsc, RwLock};

use crate::access::{AccessRegistry, SpeakerAccess};
use crate::bridge::SpeakerCastBridge;
use crate::ingress::{airplay_db_to_percent, AirPlayEvent, AirPlayIngress};
use crate::pcm::PcmHub;
use crate::speaker::{
    find_device_by_miot_did, refresh_micoapi, MinaAccount, MinaAccountView, MinaClient, MinaDevice,
    MinaCredStore, QrSessionStatus, QrSessionStore, SpeakerControl,
};
use crate::{Result, SpeakerCastError};

/// 投送服务
pub struct SpeakerCastService {
    pub access: AccessRegistry,
    pub creds: MinaCredStore,
    pub bridge: Arc<SpeakerCastBridge>,
    pub airplay: Arc<AirPlayIngress>,
    pub pcm: Arc<PcmHub>,
    accounts: RwLock<Vec<MinaAccount>>,
    qr: QrSessionStore,
}

impl SpeakerCastService {
    /// 创建并加载凭据；监听 AirPlay 开流 / 音量
    pub async fn new(
        access: AccessRegistry,
        creds: MinaCredStore,
        pcm: Arc<PcmHub>,
        bridge: Arc<SpeakerCastBridge>,
        airplay: Arc<AirPlayIngress>,
        mut event_rx: mpsc::UnboundedReceiver<AirPlayEvent>,
    ) -> Result<Arc<Self>> {
        let list = creds.load_all().await?;
        let svc = Arc::new(Self {
            access,
            creds,
            bridge,
            airplay,
            pcm,
            accounts: RwLock::new(list),
            qr: QrSessionStore::new(),
        });
        let worker = Arc::clone(&svc);
        tokio::spawn(async move {
            let mut last_vol: std::collections::HashMap<String, u8> =
                std::collections::HashMap::new();
            while let Some(ev) = event_rx.recv().await {
                match ev {
                    AirPlayEvent::SessionStart { did } => {
                        if let Err(e) = worker.on_airplay_session_start(&did).await {
                            warn!("play_url after AirPlay session failed did={did}: {e}");
                        }
                    }
                    AirPlayEvent::Volume { did, db } => {
                        let pct = airplay_db_to_percent(db);
                        if last_vol.get(&did) == Some(&pct) {
                            continue;
                        }
                        last_vol.insert(did.clone(), pct);
                        if let Err(e) = worker.set_volume(&did, pct).await {
                            warn!("AirPlay volume → MiNA failed did={did} db={db} pct={pct}: {e}");
                        } else {
                            info!("AirPlay volume → MiNA did={did} db={db:.1} pct={pct}");
                        }
                    }
                }
            }
        });
        Ok(svc)
    }

    async fn reload_accounts(&self) -> Result<()> {
        let list = self.creds.load_all().await?;
        *self.accounts.write().await = list;
        Ok(())
    }

    /// AirPlay 真正开流后：桥接 URL 交给小爱拉流
    async fn on_airplay_session_start(&self, did: &str) -> Result<()> {
        self.bridge.ensure_started().await?;
        let host = self.bridge.resolve_host().await;
        let url = self.bridge.stream_url_for(did, &host).await;
        info!("AirPlay session → play_url did={did} url={url}");
        self.test_play(did, &url).await
    }

    /// 是否有可用账号
    pub async fn mina_ready(&self) -> bool {
        self.accounts.read().await.iter().any(|a| a.is_ready())
    }

    /// 账号列表（脱敏）
    pub async fn list_accounts(&self) -> Vec<MinaAccountView> {
        self.accounts
            .read()
            .await
            .iter()
            .map(MinaAccountView::from)
            .collect()
    }

    fn client_from_account(acc: &MinaAccount) -> Result<MinaClient> {
        if !acc.is_ready() {
            return Err(SpeakerCastError::Mina("账号凭据无效".into()));
        }
        Ok(MinaClient::from_tokens(
            acc.user_id.clone(),
            acc.service_token.clone(),
        ))
    }

    async fn account(&self, user_id: &str) -> Result<MinaAccount> {
        self.accounts
            .read()
            .await
            .iter()
            .find(|a| a.user_id == user_id)
            .cloned()
            .ok_or_else(|| SpeakerCastError::Mina(format!("账号不存在: {user_id}")))
    }

    /// 扫码 begin
    pub async fn mina_qr_begin(&self) -> Result<(String, String, String)> {
        self.qr.begin().await
    }

    /// 扫码 poll；成功写入多账号表
    pub async fn mina_qr_poll(&self, session_id: &str) -> Result<QrSessionStatus> {
        let st = self.qr.poll(session_id).await?;
        if let QrSessionStatus::Ok(login) = &st {
            let acc = MinaAccount::from_login(login, "");
            self.creds.upsert(acc).await?;
            self.reload_accounts().await?;
        }
        Ok(st)
    }

    /// 手填/导入单票（upsert）
    pub async fn upsert_account_tokens(
        &self,
        user_id: String,
        service_token: String,
        pass_token: String,
    ) -> Result<MinaAccountView> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let acc = MinaAccount {
            user_id: user_id.clone(),
            service_token,
            pass_token,
            nickname: user_id,
            expire_at_ms: now + 30 * 24 * 3600 * 1000,
            created_at_ms: now,
            ..Default::default()
        };
        let saved = self.creds.upsert(acc).await?;
        self.reload_accounts().await?;
        Ok(MinaAccountView::from(&saved))
    }

    /// passToken 刷新
    pub async fn refresh_account(&self, user_id: &str) -> Result<MinaAccountView> {
        let old = self.account(user_id).await?;
        let login = refresh_micoapi(&old.pass_token, &old.user_id).await?;
        let mut acc = MinaAccount::from_login(&login, &old.nickname);
        acc.created_at_ms = old.created_at_ms;
        if acc.pass_token.is_empty() {
            acc.pass_token = old.pass_token;
        }
        let saved = self.creds.upsert(acc).await?;
        self.reload_accounts().await?;
        Ok(MinaAccountView::from(&saved))
    }

    /// 删除账号并解绑音箱
    pub async fn delete_account(&self, user_id: &str) -> Result<()> {
        let dids = self.access.disconnect_by_account(user_id).await?;
        for did in dids {
            self.airplay.stop(&did).await;
        }
        if !self.creds.remove(user_id).await? {
            return Err(SpeakerCastError::Mina(format!("账号不存在: {user_id}")));
        }
        self.reload_accounts().await?;
        Ok(())
    }

    /// 某账号下 MiNA 音箱
    pub async fn list_mina_devices_for(&self, user_id: &str) -> Result<Vec<MinaDevice>> {
        let acc = self.account(user_id).await?;
        let mut client = Self::client_from_account(&acc)?;
        client.device_list().await
    }

    /// 音响接入
    pub async fn connect(
        &self,
        did: &str,
        name: &str,
        mina_device_id: Option<&str>,
        airplay_name: Option<String>,
        account_id: &str,
    ) -> Result<SpeakerAccess> {
        let did = did.trim();
        let account_id = account_id.trim();
        let _ = self.account(account_id).await?;
        let mina_id = if let Some(id) = mina_device_id.map(str::trim).filter(|s| !s.is_empty()) {
            id.to_string()
        } else {
            match self.list_mina_devices_for(account_id).await {
                Ok(list) => find_device_by_miot_did(&list, did)
                    .map(|d| d.device_id.clone())
                    .unwrap_or_default(),
                Err(e) => {
                    log::warn!("SpeakerCast connect: MiNA 解析跳过 did={did}: {e}");
                    String::new()
                }
            }
        };
        let row = self
            .access
            .connect(did, name, &mina_id, airplay_name, account_id)
            .await?;
        info!(
            "SpeakerCast connected did={} mina={} account={}",
            row.did, row.mina_device_id, row.account_id
        );
        Ok(row)
    }

    /// 取消接入并停 AirPlay
    pub async fn disconnect(&self, did: &str) -> Result<()> {
        self.disable_airplay(did).await.ok();
        self.access.disconnect(did).await
    }

    /// 开 AirPlay
    pub async fn enable_airplay(&self, did: &str) -> Result<SpeakerAccess> {
        let row = self.access.set_airplay(did, true).await?;
        self.bridge.ensure_started().await?;
        let _ = self.pcm.ensure(&row.did, 44100, 2).await;
        if let Some(ip) = crate::bridge::detect_lan_ipv4() {
            self.bridge.set_lan_hint(ip).await;
        }
        self.airplay.start(&row).await;
        Ok(row)
    }

    /// 关 AirPlay
    pub async fn disable_airplay(&self, did: &str) -> Result<SpeakerAccess> {
        self.airplay.stop(did).await;
        self.access.set_airplay(did, false).await
    }

    /// 启动时按表恢复
    pub async fn restore(&self) -> Result<()> {
        let list = self.access.list().await;
        if list.iter().any(|s| s.airplay_enabled) {
            self.bridge.ensure_started().await?;
            if let Some(ip) = crate::bridge::detect_lan_ipv4() {
                self.bridge.set_lan_hint(ip).await;
            }
        }
        for s in list {
            if s.airplay_enabled {
                let _ = self.pcm.ensure(&s.did, 44100, 2).await;
                self.airplay.start(&s).await;
            }
        }
        Ok(())
    }

    async fn client_for_speaker(&self, row: &SpeakerAccess) -> Result<MinaClient> {
        let uid = if row.account_id.is_empty() {
            self.accounts
                .read()
                .await
                .first()
                .map(|a| a.user_id.clone())
                .unwrap_or_default()
        } else {
            row.account_id.clone()
        };
        let acc = self.account(&uid).await?;
        Self::client_from_account(&acc)
    }

    /// 对已接入音箱试播 URL
    pub async fn test_play(&self, did: &str, url: &str) -> Result<()> {
        let row = self
            .access
            .get(did)
            .await
            .ok_or_else(|| SpeakerCastError::Session(format!("未接入: {did}")))?;
        let mut client = self.client_for_speaker(&row).await?;
        let _ = client.device_list().await?;
        SpeakerControl::new(client, row.mina_device_id)
            .play_url(url)
            .await
    }

    /// 停止播放
    pub async fn test_stop(&self, did: &str) -> Result<()> {
        let row = self
            .access
            .get(did)
            .await
            .ok_or_else(|| SpeakerCastError::Session(format!("未接入: {did}")))?;
        let mut client = self.client_for_speaker(&row).await?;
        let _ = client.device_list().await?;
        SpeakerControl::new(client, row.mina_device_id)
            .stop()
            .await
    }

    /// 设置音箱音量 0–100（AirPlay / 调试）
    pub async fn set_volume(&self, did: &str, volume: u8) -> Result<()> {
        let row = self
            .access
            .get(did)
            .await
            .ok_or_else(|| SpeakerCastError::Session(format!("未接入: {did}")))?;
        if row.mina_device_id.is_empty() {
            return Err(SpeakerCastError::Session(format!(
                "未绑定 MiNA deviceId: {did}"
            )));
        }
        let client = self.client_for_speaker(&row).await?;
        SpeakerControl::new(client, row.mina_device_id)
            .set_volume(volume.min(100))
            .await
    }
}
