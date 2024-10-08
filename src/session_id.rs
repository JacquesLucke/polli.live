use crate::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionID(pub String);

impl SessionID {
    pub fn from_string(s: &str) -> Result<SessionID, AppError> {
        if s.is_empty() {
            Err(AppError::BadSessionID)
        } else if s.len() > 100 {
            Err(AppError::BadSessionID)
        } else {
            Ok(SessionID(s.to_string()))
        }
    }
}
