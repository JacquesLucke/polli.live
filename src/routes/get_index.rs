use actix_web::{HttpResponse, Responder, get};

use crate::{errors::AppError, static_files};

#[get("/")]
async fn get_index_route() -> Result<impl Responder, AppError> {
    Ok(HttpResponse::Ok()
        .content_type("text/html")
        .body(static_files::get("index.html")))
}
