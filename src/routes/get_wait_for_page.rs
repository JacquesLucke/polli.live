#![deny(clippy::unwrap_used)]

use actix_web::{Responder, get, web};

use crate::{SessionID, SharedState, errors::AppError};

#[derive(serde::Deserialize)]
struct QueryParams {
    session: String,
}

#[get("/wait_for_new_page")]
async fn get_wait_for_page_route(
    query: web::Query<QueryParams>,
    shared_state: web::Data<SharedState>,
) -> Result<impl Responder, AppError> {
    let session_id = SessionID::from_string(&query.session)?;

    let notifier = {
        let state = shared_state.state.lock();
        match state.sessions.get(&session_id) {
            None => return Err(AppError::SessionIDDoesNotExist),
            Some(session) => session.page_notifier.clone(),
        }
    };

    tokio::select! {
        _ = notifier.notified() => Ok("reload"),
        _ = tokio::time::sleep(shared_state.settings.page_update_long_poll_duration) => Ok("wait")
    }
}
