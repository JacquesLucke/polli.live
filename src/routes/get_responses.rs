#![deny(clippy::unwrap_used)]

use actix_web::{HttpResponse, Responder, get, web};
use std::collections::HashMap;

use crate::{SessionID, SharedState, UserID, errors::AppError};

#[derive(serde::Deserialize)]
struct GetResponsesParams {
    session: String,
    start: usize,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct RetrievedResponses {
    pub next_start: usize,
    pub responses_by_user: HashMap<UserID, String>,
}

#[get("/responses")]
async fn get_responses_route(
    query: web::Query<GetResponsesParams>,
    shared_state: web::Data<SharedState>,
) -> Result<impl Responder, AppError> {
    let session_id = SessionID::from_string(&query.session)?;

    let (notifier, next_response_id) = {
        let state = shared_state.state.lock();
        match state.sessions.get(&session_id) {
            None => return Err(AppError::SessionIDDoesNotExist),
            Some(session) => (session.response_notifier.clone(), session.next_response_id),
        }
    };

    // Long-poll if there are no new responses available already.
    if next_response_id <= query.start
        && !shared_state.settings.response_long_poll_duration.is_zero()
    {
        // Don't wait for notifier while session the mutex is locked!
        tokio::select! {
            _ = notifier.notified() => {},
            _ = tokio::time::sleep(shared_state.settings.response_long_poll_duration) => {},
        }
    }
    let mut state = shared_state.state.lock();
    match state.sessions.get_mut(&session_id) {
        None => Err(AppError::SessionIDDoesNotExist),
        Some(session) => {
            session.session_used();
            let mut response = RetrievedResponses {
                next_start: session.next_response_id,
                responses_by_user: HashMap::new(),
            };
            for (user_id, user_response) in session.responses.iter_mut() {
                if user_response.id < query.start {
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
