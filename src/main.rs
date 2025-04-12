#![deny(clippy::unwrap_used)]

use byte_unit::Byte;
use clap::Parser;
use parking_lot::Mutex;
use std::net::TcpListener;
use std::sync::Arc;

mod access_token;
mod cleanup;
mod errors;
mod routes;
mod session_id;
mod settings;
mod start_server;
mod state;
mod static_files;
mod user_id;

use access_token::AccessToken;
use anyhow::Result;
use errors::AppError;
use session_id::SessionID;
use settings::Settings;
use state::{SessionState, SharedState, State, UserResponse};
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

    #[arg(long, default_value = "1mb")]
    page_size_limit: Byte,

    #[arg(long, default_value = "4kb")]
    response_size_limit: Byte,
}

#[actix_web::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let listener = TcpListener::bind((args.host.clone(), args.port)).expect("Cannot bind to port");
    let Ok(local_addr) = listener.local_addr() else {
        return Err(anyhow::anyhow!("Cannot get local address"));
    };
    let actual_port = local_addr.port();

    println!("Start server on http://{}:{}", args.host, actual_port);

    let root_url = args
        .root_url
        .unwrap_or_else(|| format!("http://127.0.0.1:{}", actual_port));

    let mut settings = Settings::default(root_url);
    settings.max_page_size = args.page_size_limit;
    settings.max_response_size = args.response_size_limit;

    let state = Arc::new(Mutex::new(State {
        ..Default::default()
    }));

    let settings_clone = settings.clone();
    let state_clone = state.clone();
    tokio::spawn(async move {
        cleanup::do_periodic_cleanup(settings_clone, state_clone).await;
    });

    start_server::start_server(listener, settings, state).await?;
    Ok(())
}
