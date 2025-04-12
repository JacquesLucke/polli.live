#![deny(clippy::unwrap_used)]

use actix_web::{HttpResponse, Responder, get, web};

use crate::{SessionID, SharedState, errors::AppError, static_files};

#[derive(serde::Deserialize)]
struct Params {
    session: String,
}

#[get("/page")]
async fn get_page_route(
    query: web::Query<Params>,
    shared_state: web::Data<SharedState>,
) -> Result<impl Responder, AppError> {
    let session_id = SessionID::from_string(&query.session)?;
    let state = shared_state.state.lock();
    match state.sessions.get(&session_id) {
        None => Ok(HttpResponse::NotFound()
            .body(static_files::get("empty_session_page.html").expect("valid"))),
        Some(session) => Ok(HttpResponse::Ok().body(session.page.clone())),
    }
}
