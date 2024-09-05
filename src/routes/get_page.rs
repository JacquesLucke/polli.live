use actix_web::{get, web, HttpResponse, Responder};

use crate::{errors::AppError, static_files, SessionID, SharedState};

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
        None => Ok(HttpResponse::NotFound().body(static_files::get("empty_session_page.html"))),
        Some(session) => Ok(HttpResponse::Ok().body(session.page.clone())),
    }
}
