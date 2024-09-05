use actix_cors::Cors;
use actix_web::http::header::{CacheControl, CacheDirective};
use actix_web::middleware::DefaultHeaders;
use actix_web::{web, App, HttpServer};
use parking_lot::Mutex;
use std::net::TcpListener;
use std::sync::Arc;

use crate::{routes, Settings, SharedState, State};

pub async fn start_server(
    listener: TcpListener,
    settings: Settings,
    state: Arc<Mutex<State>>,
) -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(SharedState {
                settings: settings.clone(),
                state: state.clone(),
            }))
            .wrap(DefaultHeaders::new().add(CacheControl(vec![CacheDirective::NoCache])))
            .wrap(Cors::permissive())
            .service(routes::get_index_route)
            .service(routes::get_page_route)
            .service(routes::set_page_route)
            .service(routes::get_responses_route)
            .service(routes::post_respond_route)
            .service(routes::post_init_session_route)
            .service(routes::get_wait_for_page_route)
    })
    .workers(1)
    .listen(listener)
    .unwrap()
    .run()
    .await
}
