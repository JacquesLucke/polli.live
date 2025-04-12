use actix_web::{HttpResponse, Responder, post, web};
use rand::Rng;

use crate::{SharedState, errors::AppError, static_files};

#[derive(serde::Deserialize)]
struct DesiredSession {
    session: String,
    token: String,
}

#[derive(serde::Serialize)]
struct InitSessionResponse {
    session: String,
    token: String,
}

#[post("/new")]
async fn post_init_session_route(
    req_body: String,
    shared_state: web::Data<SharedState>,
) -> Result<impl Responder, AppError> {
    let mut session_id_length = 6;
    let mut next: DesiredSession =
        serde_json::from_str(&req_body).unwrap_or_else(|_| DesiredSession {
            session: make_random_session_id(session_id_length),
            token: make_random_access_token(),
        });
    let retries = 5;
    let initial_page = static_files::get("initial_session_page.html");

    for retry_i in 0..retries {
        // Todo, safely handle root url.
        let url = format!(
            "{}/page?session={}&notify=false",
            shared_state.settings.root_url, next.session
        );
        let client = reqwest::Client::new();
        match client
            .post(url)
            .bearer_auth(&next.token)
            .body(initial_page)
            .send()
            .await
        {
            Err(_) => {
                return Err(AppError::ServerError);
            }
            Ok(res) => {
                if res.status() == reqwest::StatusCode::OK {
                    return Ok(HttpResponse::Ok().json(InitSessionResponse {
                        session: next.session,
                        token: next.token,
                    }));
                }
            }
        }

        if retry_i > 2 {
            // Increase session id length to increase likelyness to find one that is free.
            session_id_length += 1;
        }

        next.session = make_random_session_id(session_id_length);
        next.token = make_random_access_token();
    }

    Err(AppError::ServerError)
}

fn make_random_session_id(length: usize) -> String {
    let mut rng = rand::rng();
    (0..length)
        .map(|_| rng.random_range(0..10).to_string())
        .collect()
}

fn make_random_access_token() -> String {
    let mut buf = [0u8; 32];
    if getrandom::fill(&mut buf).is_err() {
        panic!("Cannot generate random access tokens");
    }
    hex::encode(buf)
}
