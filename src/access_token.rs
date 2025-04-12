#![deny(clippy::unwrap_used)]

use crate::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct AccessToken(pub String);

impl AccessToken {
    pub fn from_string(s: &str) -> Result<AccessToken, AppError> {
        if s.len() < 10 || s.len() > 100 {
            Err(AppError::BadAccessToken)
        } else {
            Ok(AccessToken(s.to_string()))
        }
    }
}
