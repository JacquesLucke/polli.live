#![deny(clippy::unwrap_used)]

use byte_unit::{Byte, Unit};
use std::time::Duration;

#[derive(Clone)]
pub struct Settings {
    pub token_timeout: Duration,
    pub response_long_poll_duration: Duration,
    pub page_update_long_poll_duration: Duration,
    pub max_response_size: Byte,
    pub max_page_size: Byte,
    pub cleanup_interval: Duration,
    pub session_keep_alive_duration: Duration,
    pub max_memory_usage: Byte,
    pub root_url: String,
}

impl Settings {
    pub fn default(root_url: String) -> Self {
        Settings {
            token_timeout: Duration::from_secs(60 * 60 * 24),
            response_long_poll_duration: Duration::from_secs(5),
            page_update_long_poll_duration: Duration::from_secs(30),
            max_page_size: Byte::from_u64_with_unit(1, Unit::MB).expect("valid"),
            max_response_size: Byte::from_u64_with_unit(4, Unit::KB).expect("valid"),
            cleanup_interval: Duration::from_secs(3),
            session_keep_alive_duration: Duration::from_secs(24 * 60 * 60),
            max_memory_usage: Byte::from_u64_with_unit(500, Unit::MB).expect("valid"),
            root_url,
        }
    }
}
