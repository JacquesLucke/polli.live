#![deny(clippy::unwrap_used)]

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Notify;

use crate::{AccessToken, SessionID, Settings, UserID};

pub struct SharedState {
    pub settings: Settings,
    pub state: Arc<Mutex<State>>,
}

#[derive(Default)]
pub struct State {
    pub sessions: HashMap<SessionID, SessionState>,
}

pub struct SessionState {
    pub response_notifier: Arc<Notify>,
    pub page_notifier: Arc<Notify>,
    pub page: String,
    pub responses: HashMap<UserID, UserResponse>,
    pub access_token: AccessToken,
    pub next_response_id: usize,
    pub last_request: DateTime<Utc>,
}

pub struct UserResponse {
    pub data: String,
    pub id: usize,
    pub was_received: bool,
    pub time: DateTime<Utc>,
}

impl SessionState {
    pub fn new(access_token: AccessToken, page: String) -> SessionState {
        SessionState {
            response_notifier: Arc::new(Notify::new()),
            page_notifier: Arc::new(Notify::new()),
            page,
            responses: HashMap::new(),
            access_token,
            next_response_id: 0,
            last_request: Utc::now(),
        }
    }

    pub fn update(&mut self, page: String) {
        self.page = page;
        self.responses.clear();
        self.session_used();
    }

    pub fn session_used(&mut self) {
        self.last_request = Utc::now();
    }
}
