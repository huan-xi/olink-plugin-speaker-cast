//! 媒体接入协议（本期 AirPlay）。

mod airplay;
mod volume;

pub use airplay::{AirPlayEvent, AirPlayIngress, AirPlayReceiverStatus};
pub use volume::airplay_db_to_percent;
