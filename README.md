# olink-plugin-speaker-cast

开源**服务型**插件：`domain=speaker_cast`。米家音箱 AirPlay 接收 → PCM 编码 → LAN HTTP → MiNA `play_url`。

本目录为**独立 Git 仓库**（不进主仓 Cargo workspace）。AirPlay 实现内嵌于 `crates/shairplay`。

主程序（闭源）只通过 `olink-plugin-host` 安装/监督/proxy。

## 构建与打包

二进制目录必须对齐 Host：`bin/{os}-{arch}/plugin`  
（`os`/`arch` = Rust `std::env::consts`，例如 `linux-aarch64`、`macos-aarch64`，**不是** `darwin`）。

```bash
# 仅本机架构
./scripts/pack.sh
# → dist/speaker_cast-1.0.0.olinkpkg

# 同一包打多架构（需已 rustup target / 交叉工具链）
PACK_TARGETS=linux-aarch64,macos-aarch64 ./scripts/pack.sh
```

管理台「插件」上传 `.olinkpkg` → 启用。Hello：`features: ["log"]`，`features_required: []`。

## 通道

| 路径 | 用途 |
|---|---|
| `/api/plugins/proxy/speaker_cast/*` | 管理 API（登录后） |
| 插件进程自绑 `0.0.0.0:随机` | 小爱拉流直播 URL（不经 Host） |

## 数据迁移

首次启动若插件 `data/speaker-cast/` 无账号/接入文件，会从 Host `{data}/speaker-cast/` 复制。

## 规格

见主仓：`olink/docs/superpowers/specs/olink-plugin/2026-11-10-speaker-cast-service-plugin-design.md`
