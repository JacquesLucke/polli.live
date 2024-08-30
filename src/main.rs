use actix_cors::Cors;
use actix_web::http::header::ContentType;
use actix_web::http::StatusCode;
use actix_web::{error, get, web, App, HttpResponse, HttpServer, Responder};
use chrono::{DateTime, Utc};
use clap::Parser;
use derive_more::derive::{Display, Error};
use include_dir::include_dir;
use std::time::Duration;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::Notify;

static STATIC_FILES: include_dir::Dir = include_dir!("static");

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    #[arg(long, default_value = "0.0.0.0")]
    host: String,

    #[arg(long, default_value = "8000")]
    port: u16,
}

#[derive(Debug, Display, Error)]
enum AppError {
    BadUserID,
    BadSessionID,
    BadAccessToken,
    SessionIDDoesNotExist,
    ServerError,
    PageTooLarge,
}

impl error::ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::html())
            .body(self.to_string())
    }

    fn status_code(&self) -> actix_web::http::StatusCode {
        match *self {
            AppError::BadUserID => StatusCode::BAD_REQUEST,
            AppError::BadSessionID => StatusCode::BAD_REQUEST,
            AppError::SessionIDDoesNotExist => StatusCode::BAD_REQUEST,
            AppError::ServerError => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::BadAccessToken => StatusCode::BAD_REQUEST,
            AppError::PageTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        }
    }
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
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
        if s.len() < 15 {
            Err(AppError::BadAccessToken)
        } else if s.len() > 100 {
            Err(AppError::BadAccessToken)
        } else {
            Ok(AccessToken(s.to_string()))
        }
    }
}

#[derive(serde::Deserialize)]
struct QueryParams {
    session: Option<String>,
}

struct SharedState {
    token_timeout: Duration,
    state: Mutex<State>,
}

struct State {
    sessions: HashMap<SessionID, SessionState>,
}

struct SessionState {
    response_notifier: Arc<Notify>,
    page: String,
    responses: HashMap<UserID, UserResponse>,
    access_token: AccessToken,
    next_response_id: i64,
    last_access_token_use: DateTime<Utc>,
}

struct UserResponse {
    data: String,
    id: i64,
}

#[get("/")]
async fn index() -> impl Responder {
    let index_html = STATIC_FILES.get_file("index.html").unwrap();
    let content = index_html.contents_utf8().unwrap();
    HttpResponse::Ok().content_type("text/html").body(content)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    println!("Start server on http://{}:{}", args.host, args.port);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(SharedState {
                token_timeout: Duration::from_secs(60 * 60 * 24),
                state: Mutex::new(State {
                    sessions: HashMap::new(),
                }),
            }))
            .wrap(Cors::permissive())
            .service(index)
    })
    .workers(1)
    .bind((args.host, args.port))?
    .run()
    .await
}
