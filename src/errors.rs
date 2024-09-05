use actix_web::{
    http::{header::ContentType, StatusCode},
    HttpResponse,
};
use derive_more::derive::{Display, Error};

#[derive(Debug, Display, Error)]
pub enum AppError {
    BadUserID,
    BadSessionID,
    BadAccessToken,
    SessionIDDoesNotExist,
    PageTooLarge,
    ResponseTooLarge,
    ServerError,
}

impl actix_web::error::ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code())
            .insert_header(ContentType::html())
            .body(self.to_string())
    }

    fn status_code(&self) -> actix_web::http::StatusCode {
        match *self {
            AppError::BadUserID => StatusCode::BAD_REQUEST,
            AppError::BadSessionID => StatusCode::BAD_REQUEST,
            AppError::SessionIDDoesNotExist => StatusCode::NOT_FOUND,
            AppError::BadAccessToken => StatusCode::UNAUTHORIZED,
            AppError::PageTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::ResponseTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::ServerError => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
