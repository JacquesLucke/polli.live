use byte_unit::Byte;
use chrono::Utc;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;

use crate::{SessionState, Settings, State, UserResponse};

pub async fn do_periodic_cleanup(settings: Settings, state: Arc<Mutex<State>>) {
    let mut interval = tokio::time::interval(settings.cleanup_interval);
    loop {
        interval.tick().await;
        let mut state = state.lock();
        let now = Utc::now();

        // Delete old sessions.
        state
            .sessions
            .retain(|_, session| session.last_request + settings.session_keep_alive_duration > now);

        // Count used memory with a safety buffer in case more drastic measures to free
        // memory have to be taken.
        let used_bytes = get_memory_usage_with_safety_buffer(&state);
        if used_bytes < settings.max_memory_usage {
            // Enough memory is available. No need to do anything else.
            continue;
        }

        // Free responses that should have been received by all interested parties already.
        for session in state.sessions.values_mut() {
            session.responses.retain(|_, user_response| {
                user_response.was_received && user_response.time + Duration::from_secs(30) > now
            });
        }

        let used_bytes = get_memory_usage_with_safety_buffer(&state);
        if used_bytes < settings.max_memory_usage {
            // Looks like nothing else has to be freed.
            continue;
        }

        // If all above did not help, it's likely that there is some kind of attack.
        // It's not really something we can protect against at this level. Best we
        // can do is to just free everything that wasn't used a few seconds ago.
        // Valid users should use this system in real-time and should have received
        // responses in less than a few seconds already.
        state
            .sessions
            .retain(|_, session| session.last_request + Duration::from_secs(5) > now);
        state.sessions.shrink_to_fit();
        for session in state.sessions.values_mut() {
            session.responses.shrink_to_fit();
        }
    }
}

fn get_memory_usage_with_safety_buffer(state: &State) -> Byte {
    count_user_memory_usage(state).multiply(2).unwrap()
}

fn count_user_memory_usage(state: &State) -> Byte {
    let mut used_bytes: usize = 0;
    for (session_id, session) in &state.sessions {
        used_bytes += session_id.0.len() + session.page.len() + session.access_token.0.len();
        for (user_id, user_response) in &session.responses {
            used_bytes += user_id.0.len() + user_response.data.len();
        }
        used_bytes += size_of::<UserResponse>() * session.responses.capacity();
    }
    used_bytes += size_of::<SessionState>() * state.sessions.capacity();
    Byte::from_u64(used_bytes as u64)
}
