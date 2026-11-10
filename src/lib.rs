//! 开源音箱投送插件业务库（AirPlay → 小爱 play_url，不接 HAP）。

pub mod access;
pub mod api;
pub mod api_resp;
pub mod bootstrap;
pub mod bridge;
pub mod error;
pub mod ingress;
pub mod migrate;
pub mod pcm;
pub mod runtime;
pub mod service;
pub mod session;
pub mod speaker;

pub use error::{Result, SpeakerCastError};
pub use session::{MediaPayload, MediaSessionId, MediaSessionSink, TrackMeta};
