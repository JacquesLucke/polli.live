use actix_cors::Cors;
use actix_web::http::header::{CacheControl, CacheDirective};
use actix_web::middleware::DefaultHeaders;
use actix_web::{web, App, HttpServer};
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
mod static_files;

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

async fn start_server(
    listener: TcpListener,
    settings: Settings,
    state: Arc<Mutex<State>>,
) -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(SharedState {
                settings: settings.clone(),
                state: state.clone(),
            }))
            .wrap(DefaultHeaders::new().add(CacheControl(vec![CacheDirective::NoCache])))
            .wrap(Cors::permissive())
            .service(routes::get_index_route)
            .service(routes::get_page_route)
            .service(routes::set_page_route)
            .service(routes::get_responses_route)
            .service(routes::post_respond_route)
            .service(routes::post_init_session_route)
            .service(routes::get_wait_for_page_route)
    })
    .workers(1)
    .listen(listener)
    .unwrap()
    .run()
    .await
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

    start_server(listener, settings, state).await
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
                None => format!("{}/page", &self.url),
                Some(session_id) => format!("{}/page?session={}", &self.url, session_id),
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
        let url = format!("http://127.0.0.1:{}", port);

        let url_clone = url.clone();
        let server = tokio::spawn(async move {
            start_server(
                listener,
                Settings::default(url_clone),
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
            url: url,
            client: reqwest::Client::new(),
        }
    }

    #[tokio::test]
    async fn static_index_page() {
        let ctx = setup().await;
        let res = ctx.request_static_page("/").await;
        assert_eq!(res.text().await.unwrap(), static_files::get("index.html"));
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
        let result: routes::RetrievedResponses = res.json().await.unwrap();
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
