use byte_unit::{Byte, Unit};
use chrono::{DateTime, Utc};
use clap::Parser;
use parking_lot::Mutex;
use std::net::TcpListener;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Notify;

mod access_token;
mod cleanup;
mod errors;
mod routes;
mod session_id;
mod settings;
mod start_server;
mod static_files;
mod user_id;

use access_token::AccessToken;
use errors::AppError;
use session_id::SessionID;
use settings::Settings;
use user_id::UserID;

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

struct UserResponse {
    data: String,
    id: usize,
    was_received: bool,
    time: DateTime<Utc>,
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
