mod get_index;
mod get_page;
mod get_responses;
mod get_wait_for_page;
mod post_init_session;
mod post_respond;
mod set_page;

pub use get_index::get_index_route;
pub use get_page::get_page_route;
pub use get_responses::get_responses_route;
pub use get_wait_for_page::get_wait_for_page_route;
pub use post_init_session::post_init_session_route;
pub use post_respond::post_respond_route;
pub use set_page::set_page_route;

#[cfg(test)]
pub use get_responses::RetrievedResponses;
