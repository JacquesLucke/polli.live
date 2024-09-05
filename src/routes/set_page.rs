use actix_web::{post, web, Responder};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use byte_unit::Byte;
use chrono::Utc;

use crate::{errors::AppError, static_files, AccessToken, SessionID, SessionState, SharedState};

#[derive(serde::Deserialize)]
struct SetPageQueryParams {
    session: String,
    notify: Option<bool>,
}

#[post("/page")]
async fn set_page_route(
    mut page: String,
    query: web::Query<SetPageQueryParams>,
    shared_state: web::Data<SharedState>,
    auth: BearerAuth,
) -> Result<impl Responder, AppError> {
    let access_token = AccessToken::from_string(auth.token())?;
    let session_id = SessionID::from_string(&query.session)?;

    if Byte::from_u64(page.len() as u64) > shared_state.settings.max_page_size {
        return Err(AppError::PageTooLarge);
    }

    match page.find("</head>") {
        None => {}
        Some(idx) => {
            page.insert_str(idx, &static_files::get("polli_live_injection.html"));
        }
    }

    let mut state = shared_state.state.lock();
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
            if query.notify.unwrap_or(true) {
                session.page_notifier.notify_waiters();
            }
        }
    }
    Ok("Page updated.")
}
