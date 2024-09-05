use byte_unit::{Byte, Unit};
use chrono::{DateTime, Utc};
use clap::Parser;
use errors::AppError;
use parking_lot::Mutex;
use std::net::TcpListener;
use std::time::Duration;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Notify;

mod cleanup;
mod errors;
mod routes;
mod start_server;
mod static_files;

#[cfg(test)]
mod tests;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    #[arg(long, default_value = "9000")]
    port: u16,

    #[arg(long)]
    root_url: Option<String>,

    #[arg(long, default_value = "1024")]
    page_size_limit_kb: usize,

    #[arg(long, default_value = "4")]
    response_size_limit_kb: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionID(String);

impl SessionID {
    fn from_string(s: &str) -> Result<SessionID, AppError> {
        if s.is_empty() {
            Err(AppError::BadSessionID)
        } else if s.len() > 100 {
            Err(AppError::BadSessionID)
        } else {
            Ok(SessionID(s.to_string()))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct UserID(String);

impl UserID {
    fn from_string(s: &str) -> Result<UserID, AppError> {
        if s.is_empty() {
            Err(AppError::BadUserID)
        } else if s.len() > 100 {
            Err(AppError::BadUserID)
        } else {
            Ok(UserID(s.to_string()))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
struct AccessToken(String);

impl AccessToken {
    fn from_string(s: &str) -> Result<AccessToken, AppError> {
        if s.len() < 10 {
            Err(AppError::BadAccessToken)
        } else if s.len() > 100 {
            Err(AppError::BadAccessToken)
        } else {
            Ok(AccessToken(s.to_string()))
        }
    }
}

struct UserResponse {
    data: String,
    id: usize,
    was_received: bool,
    time: DateTime<Utc>,
}

#[derive(Clone)]
struct Settings {
    token_timeout: Duration,
    response_long_poll_duration: Duration,
    page_update_long_poll_duration: Duration,
    max_response_size: Byte,
    max_page_size: Byte,
    cleanup_interval: Duration,
    session_keep_alive_duration: Duration,
    max_memory_usage: Byte,
    root_url: String,
}

impl Settings {
    fn default(root_url: String) -> Self {
        Settings {
            token_timeout: Duration::from_secs(60 * 60 * 24),
            response_long_poll_duration: Duration::from_secs(5),
            page_update_long_poll_duration: Duration::from_secs(30),
            max_page_size: Byte::from_u64_with_unit(1, Unit::MB).unwrap(),
            max_response_size: Byte::from_u64_with_unit(4, Unit::KB).unwrap(),
            cleanup_interval: Duration::from_secs(3),
            session_keep_alive_duration: Duration::from_secs(24 * 60 * 60),
            max_memory_usage: Byte::from_u64_with_unit(500, Unit::MB).unwrap(),
            root_url: root_url,
        }
    }
}

struct SharedState {
    settings: Settings,
    state: Arc<Mutex<State>>,
}

struct State {
    sessions: HashMap<SessionID, SessionState>,
}

impl Default for State {
    fn default() -> Self {
        State {
            sessions: HashMap::new(),
        }
    }
}

struct SessionState {
    response_notifier: Arc<Notify>,
    page_notifier: Arc<Notify>,
    page: String,
    responses: HashMap<UserID, UserResponse>,
    access_token: AccessToken,
    next_response_id: usize,
    last_request: DateTime<Utc>,
}

impl SessionState {
    fn new(access_token: AccessToken, page: String) -> SessionState {
        SessionState {
            response_notifier: Arc::new(Notify::new()),
            page_notifier: Arc::new(Notify::new()),
            page: page,
            responses: HashMap::new(),
            access_token: access_token,
            next_response_id: 0,
            last_request: Utc::now(),
        }
    }

    fn update(&mut self, page: String) {
        self.page = page;
        self.responses.clear();
        self.session_used();
    }

    fn session_used(&mut self) {
        self.last_request = Utc::now();
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let listener = TcpListener::bind((args.host.clone(), args.port)).expect("Cannot bind to port");
    let actual_port = listener.local_addr().unwrap().port();

    println!("Start server on http://{}:{}", args.host, actual_port);

    let root_url = args
        .root_url
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", actual_port));

    let mut settings = Settings::default(root_url);
    settings.max_page_size =
        Byte::from_u64_with_unit(args.page_size_limit_kb as u64, Unit::KB).unwrap();
    settings.max_response_size =
        Byte::from_u64_with_unit(args.response_size_limit_kb as u64, Unit::KB).unwrap();

    let state = Arc::new(Mutex::new(State {
        ..Default::default()
    }));

    let settings_clone = settings.clone();
    let state_clone = state.clone();
    tokio::spawn(async move {
        cleanup::do_periodic_cleanup(settings_clone, state_clone).await;
    });

    start_server::start_server(listener, settings, state).await
}
