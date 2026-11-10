//! 从主程序旧目录 `{data}/speaker-cast/` 迁入插件 data。

use std::path::{Path, PathBuf};

use log::{info, warn};

/// 推断 Host data_path：`…/plugins/speaker_cast/data` → 上三级
fn host_data_path(plugin_data: &Path) -> Option<PathBuf> {
    plugin_data
        .parent()? // speaker_cast
        .parent()? // plugins
        .parent() // data_path
        .map(|p| p.to_path_buf())
}

/// 若插件侧尚无账号/接入文件，从 Host 旧目录复制
pub async fn migrate_legacy_data(plugin_data: &Path) {
    let dest = plugin_data.join("speaker-cast");
    let need_accounts = !dest.join("accounts.json").exists() && !dest.join("mina.json").exists();
    let need_access = !dest.join("access.json").exists();
    if !need_accounts && !need_access {
        return;
    }
    let Some(host) = host_data_path(plugin_data) else {
        return;
    };
    let legacy = host.join("speaker-cast");
    if !legacy.is_dir() {
        return;
    }
    if let Err(e) = tokio::fs::create_dir_all(&dest).await {
        warn!("migrate create dest failed: {e}");
        return;
    }
    for name in ["accounts.json", "mina.json", "access.json"] {
        let src = legacy.join(name);
        let dst = dest.join(name);
        if src.is_file() && !dst.exists() {
            match tokio::fs::copy(&src, &dst).await {
                Ok(_) => info!("migrated {name} from host speaker-cast"),
                Err(e) => warn!("migrate {name} failed: {e}"),
            }
        }
    }
}
