//! 音箱投送错误类型。

use thiserror::Error;

/// 音箱投送错误
#[derive(Debug, Error)]
pub enum SpeakerCastError {
    /// MiNA / 云控失败
    #[error("mina: {0}")]
    Mina(String),
    /// 会话不存在或已结束
    #[error("session: {0}")]
    Session(String),
    /// 桥接 / 编码 / HTTP 失败
    #[error("bridge: {0}")]
    Bridge(String),
    /// 尚未实现（预留路径）
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// 网络 IO
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// HTTP 客户端
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

/// 结果别名
pub type Result<T> = std::result::Result<T, SpeakerCastError>;
