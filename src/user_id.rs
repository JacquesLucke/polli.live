#![deny(clippy::unwrap_used)]

use crate::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct UserID(pub String);

impl UserID {
    pub fn from_string(s: &str) -> Result<UserID, AppError> {
        if s.is_empty() || s.len() > 100 {
            Err(AppError::BadUserID)
        } else {
            Ok(UserID(s.to_string()))
        }
    }
}
