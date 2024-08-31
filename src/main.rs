use actix_cors::Cors;
use actix_web::http::header::{CacheControl, CacheDirective, ContentType};
use actix_web::http::StatusCode;
use actix_web::middleware::DefaultHeaders;
use actix_web::{error, get, post, web, App, HttpRequest, HttpResponse, HttpServer, Responder};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use chrono::{DateTime, Utc};
use clap::Parser;
use derive_more::derive::{Display, Error};
use include_dir::include_dir;
use std::str::FromStr;
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
struct SessionQueryParams {
    session: Option<String>,
}

struct SharedState {
    token_timeout: Duration,
    long_poll_duration: Duration,
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

impl SessionState {
    fn new(access_token: AccessToken, page: String) -> SessionState {
        SessionState {
            response_notifier: Arc::new(Notify::new()),
            page: page,
            responses: HashMap::new(),
            access_token: access_token,
            next_response_id: 0,
            last_access_token_use: Utc::now(),
        }
    }

    fn update(&mut self, page: String) {
        self.page = page;
        self.responses.clear();
        self.access_token_used();
    }

    fn access_token_used(&mut self) {
        self.last_access_token_use = Utc::now();
    }
}

struct UserResponse {
    data: String,
    id: i64,
    was_received: bool,
}

#[get("/")]
async fn index(
    query: web::Query<SessionQueryParams>,
    state: web::Data<SharedState>,
) -> Result<impl Responder, AppError> {
    match &query.session {
        None => Ok(HttpResponse::Ok()
            .content_type("text/html")
            .body(get_static_file("index.html"))),
        Some(session_id) => {
            let session_id = SessionID::from_string(&session_id)?;
            Ok(get_poll_page(session_id, state))
        }
    }
}

fn get_poll_page(session_id: SessionID, state: web::Data<SharedState>) -> HttpResponse {
    let state = state.state.lock().unwrap();
    match state.sessions.get(&session_id) {
        None => HttpResponse::NotFound().body(get_static_file("empty_session_page.html")),
        Some(session) => HttpResponse::Ok().body(session.page.clone()),
    }
}

fn get_static_file(filename: &str) -> &'static str {
    let index_html = STATIC_FILES.get_file(filename).unwrap();
    index_html.contents_utf8().unwrap()
}

#[post("/set_page")]
async fn set_page(
    page: String,
    query: web::Query<SessionQueryParams>,
    shared_state: web::Data<SharedState>,
    auth: BearerAuth,
) -> Result<impl Responder, AppError> {
    let access_token = AccessToken::from_string(auth.token())?;
    let session_id = query.session.as_ref().ok_or(AppError::BadSessionID)?;
    let session_id = SessionID::from_string(session_id)?;

    let mut state = shared_state.state.lock().unwrap();
    match state.sessions.get_mut(&session_id) {
        None => {
            state
                .sessions
                .insert(session_id, SessionState::new(access_token, page));
        }
        Some(session) => {
            if session.access_token != access_token {
                if session.last_access_token_use + shared_state.token_timeout > Utc::now() {
                    return Err(AppError::BadAccessToken);
                }
                *session = SessionState::new(access_token, page);
            } else {
                session.update(page);
            }
        }
    }
    Ok("Page updated.")
}

#[derive(serde::Deserialize)]
struct GetResponsesParams {
    session: String,
    start: Option<i64>,
}

#[derive(serde::Serialize)]
struct RetrievedResponses {
    next_start: i64,
    responses_by_user: HashMap<UserID, String>,
}

#[get("/responses")]
async fn get_responses(
    query: web::Query<GetResponsesParams>,
    shared_state: web::Data<SharedState>,
    auth: BearerAuth,
) -> Result<impl Responder, AppError> {
    let access_token = AccessToken::from_string(auth.token())?;
    let session_id = SessionID::from_string(&query.session)?;

    let mut state = shared_state.state.lock().unwrap();
    match state.sessions.get_mut(&session_id) {
        None => Err(AppError::SessionIDDoesNotExist),
        Some(session) => {
            if access_token != session.access_token {
                return Err(AppError::BadAccessToken);
            }

            tokio::select! {
                _ = session.response_notifier.notified() => {},
                _ = tokio::time::sleep(shared_state.long_poll_duration) => {},
            }

            session.access_token_used();
            let start = query.start.unwrap_or(0);
            let mut response = RetrievedResponses {
                next_start: session.next_response_id,
                responses_by_user: HashMap::new(),
            };
            for (user_id, user_response) in session.responses.iter_mut() {
                if user_response.id < start {
                    user_response.was_received = true;
                    continue;
                }
                response
                    .responses_by_user
                    .insert(user_id.clone(), user_response.data.clone());
            }
            Ok(HttpResponse::Ok().json(response))
        }
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();

    println!("Start server on http://{}:{}", args.host, args.port);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(SharedState {
                token_timeout: Duration::from_secs(60 * 60 * 24),
                long_poll_duration: Duration::from_secs(1),
                state: Mutex::new(State {
                    sessions: HashMap::new(),
                }),
            }))
            .wrap(DefaultHeaders::new().add(CacheControl(vec![CacheDirective::NoCache])))
            .wrap(Cors::permissive())
            .service(index)
            .service(set_page)
            .service(get_responses)
    })
    .workers(1)
    .bind((args.host, args.port))?
    .run()
    .await
}
