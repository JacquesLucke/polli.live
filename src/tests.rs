use std::sync::Arc;

use parking_lot::Mutex;
use std::net::TcpListener;

use crate::{Settings, State, routes, static_files, user_id::UserID};

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
            .expect("")
    }

    async fn request_session_page(&self, session_id: &str) -> reqwest::Response {
        self.client
            .get(format!("{}/page?session={}", &self.url, &session_id))
            .send()
            .await
            .expect("")
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
        if let Some(token) = token {
            builder = builder.bearer_auth(token);
        }
        builder = builder.body(page.to_string());
        builder.send().await.expect("")
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
        if let Some(session_id) = session_id {
            url.push_str(&format!("session={}&", session_id));
        }
        if let Some(user_id) = user_id {
            url.push_str(&format!("user={}", user_id));
        }
        self.client
            .post(url)
            .body(response_data.to_string())
            .send()
            .await
            .expect("")
    }

    async fn request_responses(
        &self,
        session_id: Option<&str>,
        start: Option<usize>,
    ) -> reqwest::Response {
        let mut url = self.url.clone();
        url.push_str("/responses?");
        if let Some(session_id) = session_id {
            url.push_str(&format!("session={}&", session_id));
        }
        if let Some(start) = start {
            url.push_str(&format!("start={}", start));
        }
        self.client.get(&url).send().await.expect("")
    }

    async fn request_static_page(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{}", self.url, path))
            .send()
            .await
            .expect("")
    }
}

async fn setup() -> TestContext {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind to random port");
    let port = listener.local_addr().expect("").port();
    let url = format!("http://127.0.0.1:{}", port);

    let url_clone = url.clone();
    let server = tokio::spawn(async move {
        crate::start_server::start_server(
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
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    TestContext {
        handle: server,
        url,
        client: reqwest::Client::new(),
    }
}

#[tokio::test]
async fn static_index_page() {
    let ctx = setup().await;
    let res = ctx.request_static_page("/").await;
    assert_eq!(
        res.text().await.expect(""),
        static_files::get("index.html").expect("valid")
    );
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
    assert_eq!(res.text().await.expect(""), page);
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

    let res = ctx.request_responses(Some(session), Some(0)).await;
    assert_eq!(res.status(), reqwest::StatusCode::OK);
    let result: routes::RetrievedResponses = res.json().await.expect("");
    assert_eq!(result.next_start, 1);
    assert_eq!(result.responses_by_user.len(), 1);
    assert_eq!(
        result
            .responses_by_user
            .get(&UserID::from_string(user).expect(""))
            .expect(""),
        response_data
    );
}
