pub mod discord;
pub mod manager;
pub mod secrets;
pub mod slack;
pub mod ws_manager;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEvent {
    pub source: String,
    pub pr_url: String,
    pub requester: Option<String>,
    pub channel: Option<String>,
    pub received_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelStatus {
    pub source: String,
    pub connected: bool,
    pub message: Option<String>,
}
