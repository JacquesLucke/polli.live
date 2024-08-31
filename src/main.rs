use actix_cors::Cors;
use actix_web::http::header::{CacheControl, CacheDirective, ContentType};
use actix_web::http::StatusCode;
use actix_web::middleware::DefaultHeaders;
use actix_web::{error, get, post, web, App, HttpResponse, HttpServer, Responder};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use byte_unit::{Byte, Unit};
use chrono::{DateTime, Utc};
use clap::Parser;
use derive_more::derive::{Display, Error};
use include_dir::include_dir;
use std::net::TcpListener;
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
    PageTooLarge,
    ResponseTooLarge,
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
            AppError::SessionIDDoesNotExist => StatusCode::NOT_FOUND,
            AppError::BadAccessToken => StatusCode::UNAUTHORIZED,
            AppError::PageTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::ResponseTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
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

#[derive(serde::Deserialize)]
struct SessionQueryParam {
    session: Option<String>,
}

#[derive(Copy, Clone)]
struct Settings {
    token_timeout: Duration,
    long_poll_duration: Duration,
    max_response_size: Byte,
    max_page_size: Byte,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            token_timeout: Duration::from_secs(60 * 60 * 24),
            long_poll_duration: Duration::from_secs(1),
            max_page_size: Byte::from_u64_with_unit(1, Unit::MB).unwrap(),
            max_response_size: Byte::from_u64_with_unit(4, Unit::KB).unwrap(),
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

struct UserResponse {
    data: String,
    id: usize,
    was_received: bool,
}

#[get("/")]
async fn index(
    query: web::Query<SessionQueryParam>,
    shared_state: web::Data<SharedState>,
) -> Result<impl Responder, AppError> {
    match &query.session {
        None => Ok(HttpResponse::Ok()
            .content_type("text/html")
            .body(get_static_file("index.html"))),
        Some(session_id) => {
            let session_id = SessionID::from_string(&session_id)?;
            Ok(get_poll_page(session_id, shared_state))
        }
    }
}

fn get_poll_page(session_id: SessionID, shared_state: web::Data<SharedState>) -> HttpResponse {
    let state = shared_state.state.lock().unwrap();
    match state.sessions.get(&session_id) {
        None => HttpResponse::NotFound().body(get_static_file("empty_session_page.html")),
        Some(session) => HttpResponse::Ok().body(session.page.clone()),
    }
}

fn get_static_file(filename: &str) -> &'static str {
    let index_html = STATIC_FILES.get_file(filename).unwrap();
    index_html.contents_utf8().unwrap()
}

#[derive(serde::Deserialize)]
struct RespondQueryParams {
    session: String,
    user: String,
}

#[post("/respond")]
async fn respond(
    response_data: String,
    query: web::Query<RespondQueryParams>,
    shared_state: web::Data<SharedState>,
) -> Result<impl Responder, AppError> {
    let session_id = SessionID::from_string(&query.session)?;
    let user_id = UserID::from_string(&query.user)?;

    if Byte::from_u64(response_data.len() as u64) > shared_state.settings.max_response_size {
        return Err(AppError::ResponseTooLarge);
    }

    let mut state = shared_state.state.lock().unwrap();
    match state.sessions.get_mut(&session_id) {
        None => Err(AppError::SessionIDDoesNotExist),
        Some(session) => {
            let response_id = session.next_response_id;
            session.next_response_id += 1;

            session.responses.insert(
                user_id,
                UserResponse {
                    data: response_data,
                    id: response_id,
                    was_received: false,
                },
            );
            session.response_notifier.notify_waiters();

            Ok(HttpResponse::Ok().body("Response updated."))
        }
    }
}

#[post("/set_page")]
async fn set_page(
    page: String,
    query: web::Query<SessionQueryParam>,
    shared_state: web::Data<SharedState>,
    auth: BearerAuth,
) -> Result<impl Responder, AppError> {
    let access_token = AccessToken::from_string(auth.token())?;
    let session_id = query.session.as_ref().ok_or(AppError::BadSessionID)?;
    let session_id = SessionID::from_string(session_id)?;

    if Byte::from_u64(page.len() as u64) > shared_state.settings.max_page_size {
        return Err(AppError::PageTooLarge);
    }

    let mut state = shared_state.state.lock().unwrap();
    match state.sessions.get_mut(&session_id) {
        None => {
            state
                .sessions
                .insert(session_id, SessionState::new(access_token, page));
        }
        Some(session) => {
            if session.access_token != access_token {
                if session.last_request + shared_state.settings.token_timeout > Utc::now() {
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
    start: Option<usize>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RetrievedResponses {
    next_start: usize,
    responses_by_user: HashMap<UserID, String>,
}

#[get("/responses")]
async fn get_responses(
    query: web::Query<GetResponsesParams>,
    shared_state: web::Data<SharedState>,
) -> Result<impl Responder, AppError> {
    let session_id = SessionID::from_string(&query.session)?;

    let notifier = {
        let mut state = shared_state.state.lock().unwrap();
        match state.sessions.get_mut(&session_id) {
            None => return Err(AppError::SessionIDDoesNotExist),
            Some(session) => session.response_notifier.clone(),
        }
    };
    if !shared_state.settings.long_poll_duration.is_zero() {
        // Don't wait for notifier while the mutex is locked!
        tokio::select! {
            _ = notifier.notified() => {},
            _ = tokio::time::sleep(shared_state.settings.long_poll_duration) => {},
        }
    }
    let mut state = shared_state.state.lock().unwrap();
    match state.sessions.get_mut(&session_id) {
        None => Err(AppError::SessionIDDoesNotExist),
        Some(session) => {
            session.session_used();
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

async fn start_server(
    listener: TcpListener,
    settings: Settings,
    state: Arc<Mutex<State>>,
) -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(SharedState {
                settings: settings,
                state: state.clone(),
            }))
            .wrap(DefaultHeaders::new().add(CacheControl(vec![CacheDirective::NoCache])))
            .wrap(Cors::permissive())
            .service(index)
            .service(set_page)
            .service(get_responses)
            .service(respond)
    })
    .workers(1)
    .listen(listener)
    .unwrap()
    .run()
    .await
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let mut args = Args::parse();

    let listener = TcpListener::bind((args.host.clone(), args.port)).expect("Cannot bind to port");
    let actual_port = listener.local_addr().unwrap().port();
    args.port = actual_port;

    println!("Start server on http://{}:{}", args.host, actual_port);

    start_server(
        listener,
        Settings {
            ..Default::default()
        },
        Arc::new(Mutex::new(State {
            ..Default::default()
        })),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestContext {
        handle: tokio::task::JoinHandle<()>,
        url: String,
        client: reqwest::Client,
    }

    impl Drop for TestContext {
        fn drop(&mut self) {
            self.handle.abort();
        }
    }

    impl TestContext {
        async fn request_session_page_text(&self, session_id: &str) -> String {
            self.request_session_page(session_id)
                .await
                .text()
                .await
                .unwrap()
        }

        async fn request_session_page(&self, session_id: &str) -> reqwest::Response {
            self.client
                .get(format!("{}?session={}", &self.url, &session_id))
                .send()
                .await
                .unwrap()
        }

        async fn request_page_update(
            &self,
            session_id: Option<&str>,
            token: Option<&str>,
            page: &str,
        ) -> reqwest::Response {
            let url = match session_id {
                None => format!("{}/set_page", &self.url),
                Some(session_id) => format!("{}/set_page?session={}", &self.url, session_id),
            };
            let mut builder = self.client.post(&url);
            if token.is_some() {
                builder = builder.bearer_auth(token.unwrap());
            }
            builder = builder.body(page.to_string());
            builder.send().await.unwrap()
        }

        async fn set_page_and_check(&self, session_id: &str, token: &str, page: &str) {
            let res = self
                .request_page_update(Some(session_id), Some(token), page)
                .await;
            assert_eq!(res.status(), reqwest::StatusCode::OK);
            assert_eq!(self.request_session_page_text(session_id).await, page);
        }

        async fn send_reponse(
            &self,
            session_id: Option<&str>,
            user_id: Option<&str>,
            response_data: &str,
        ) -> reqwest::Response {
            let mut url = self.url.clone();
            url.push_str("/respond?");
            if session_id.is_some() {
                url.push_str(&format!("session={}&", session_id.unwrap()));
            }
            if user_id.is_some() {
                url.push_str(&format!("user={}", user_id.unwrap()));
            }
            self.client
                .post(url)
                .body(response_data.to_string())
                .send()
                .await
                .unwrap()
        }

        async fn request_responses(&self, session_id: Option<&str>) -> reqwest::Response {
            let url = match session_id {
                None => format!("{}/responses", &self.url),
                Some(session_id) => format!("{}/responses?session={}", &self.url, session_id),
            };
            self.client.get(&url).send().await.unwrap()
        }

        async fn request_static_page(&self, path: &str) -> reqwest::Response {
            self.client
                .get(format!("{}{}", self.url, path))
                .send()
                .await
                .unwrap()
        }
    }

    async fn setup() -> TestContext {
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to random port");
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            start_server(
                listener,
                Settings {
                    ..Default::default()
                },
                Arc::new(Mutex::new(State {
                    ..Default::default()
                })),
            )
            .await
            .expect("failed to start server");
        });

        // Wait for server to start.
        tokio::time::sleep(Duration::from_millis(100)).await;

        TestContext {
            handle: server,
            url: format!("http://127.0.0.1:{}", port),
            client: reqwest::Client::new(),
        }
    }

    #[tokio::test]
    async fn static_index_page() {
        let ctx = setup().await;
        let res = ctx.request_static_page("/").await;
        assert_eq!(res.text().await.unwrap(), get_static_file("index.html"));
    }

    #[tokio::test]
    async fn not_found_session_page() {
        let ctx = setup().await;
        let res = ctx.request_session_page("1").await;
        assert_eq!(res.status(), reqwest::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn set_page_without_token() {
        let ctx = setup().await;
        let res = ctx.request_page_update(Some("1"), None, "test").await;
        assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn set_page_and_request() {
        let ctx = setup().await;

        let page = "my test page";

        let res = ctx
            .request_page_update(Some("1"), Some("my-test-token"), page)
            .await;
        assert_eq!(res.status(), reqwest::StatusCode::OK);

        let res = ctx.request_session_page("1").await;
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        assert_eq!(res.text().await.unwrap(), page);
    }

    #[tokio::test]
    async fn set_page_twice_with_same_token() {
        let ctx = setup().await;

        let session = "a";
        let token = "my-test-token";
        let page_1 = "page one";
        let page_2 = "page two";

        let res = ctx
            .request_page_update(Some(session), Some(token), "page one")
            .await;
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        assert_eq!(ctx.request_session_page_text(session).await, page_1);

        let res = ctx
            .request_page_update(Some(session), Some(token), page_2)
            .await;
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        assert_eq!(ctx.request_session_page_text(session).await, page_2);
    }

    #[tokio::test]
    async fn try_update_page_with_other_token() {
        let ctx = setup().await;

        let session = "b";
        let token_1 = "my-first-token";
        let token_2 = "my-second-token";
        let page_1 = "page 1";
        let page_2 = "page 2";

        let res = ctx
            .request_page_update(Some(session), Some(token_1), page_1)
            .await;
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        assert_eq!(ctx.request_session_page_text(session).await, page_1);

        let res = ctx
            .request_page_update(Some(session), Some(token_2), page_2)
            .await;
        assert_eq!(res.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(ctx.request_session_page_text(session).await, page_1);
    }

    #[tokio::test]
    async fn single_response() {
        let ctx = setup().await;

        let session = "c";
        let token = "my-test-token";
        let page = "test page";
        let user = "me";
        let response_data = "42";

        ctx.set_page_and_check(session, token, page).await;

        let res = ctx
            .send_reponse(Some(session), Some(user), response_data)
            .await;
        assert_eq!(res.status(), reqwest::StatusCode::OK);

        let res = ctx.request_responses(Some(&session)).await;
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        let result: RetrievedResponses = res.json().await.unwrap();
        assert_eq!(result.next_start, 1);
        assert_eq!(result.responses_by_user.len(), 1);
        assert_eq!(
            result
                .responses_by_user
                .get(&UserID::from_string(user).unwrap())
                .unwrap(),
            response_data
        );
    }
}
