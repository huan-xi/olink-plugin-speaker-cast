//! 从 data_dir 装配 SpeakerCastService。

use std::path::Path;
use std::sync::Arc;

use crate::access::AccessRegistry;
use crate::bridge::SpeakerCastBridge;
use crate::ingress::AirPlayIngress;
use crate::migrate::migrate_legacy_data;
use crate::pcm::PcmHub;
use crate::service::SpeakerCastService;
use crate::speaker::MinaCredStore;

/// 迁数据并创建服务
pub async fn build_service(data_dir: &Path) -> anyhow::Result<Arc<SpeakerCastService>> {
    migrate_legacy_data(data_dir).await;
    let access = AccessRegistry::load(data_dir).await?;
    let creds = MinaCredStore::new(data_dir);
    let pcm = PcmHub::new();
    let bridge = SpeakerCastBridge::new(Arc::clone(&pcm));
    let (airplay, event_rx) = AirPlayIngress::new(Arc::clone(&pcm));
    let service = SpeakerCastService::new(access, creds, pcm, bridge, airplay, event_rx).await?;
    Ok(service)
}
