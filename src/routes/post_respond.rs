#![deny(clippy::unwrap_used)]

use actix_web::{HttpResponse, Responder, post, web};
use byte_unit::Byte;
use chrono::Utc;

use crate::{SessionID, SharedState, UserID, UserResponse, errors::AppError};

#[derive(serde::Deserialize)]
struct RespondQueryParams {
    session: String,
    user: String,
}

#[post("/respond")]
async fn post_respond_route(
    response_data: String,
    query: web::Query<RespondQueryParams>,
    shared_state: web::Data<SharedState>,
) -> Result<impl Responder, AppError> {
    let session_id = SessionID::from_string(&query.session)?;
    let user_id = UserID::from_string(&query.user)?;

    if Byte::from_u64(response_data.len() as u64) > shared_state.settings.max_response_size {
        return Err(AppError::ResponseTooLarge);
    }

    let mut state = shared_state.state.lock();
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
                    time: Utc::now(),
                },
            );
            session.session_used();
            session.response_notifier.notify_waiters();

            Ok(HttpResponse::Ok().body("Response updated."))
        }
    }
}
