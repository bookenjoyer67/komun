mod admin;
mod reports;
// `pub(crate)` on these three only so `crate::tests::market` can unit-test their validators
// directly; nothing outside the crate can reach them.
pub(crate) mod categories;
pub(crate) mod conversations;
pub mod directory;
mod endorsements;
mod error;
mod geocode;
mod health;
mod link_preview;
mod node;
mod notifications;
pub(crate) mod posts;
pub(crate) mod reviews;
mod search;
mod users;

use axum::Router;

use crate::auth;
use crate::AppState;

pub use error::StatusError;

pub fn router(state: AppState) -> Router {
    let mut r = Router::new()
        .route("/geocode", axum::routing::get(geocode::geocode))
        .merge(health::router())
        .merge(node::router(state.clone()))
        .merge(conversations::router(state.clone()))
        .merge(notifications::router(state.clone()))
        .merge(admin::router(state.clone()))
        // Mounted flat, not nested: the two halves sit under different path prefixes and guards.
        .merge(categories::router(state.clone()))
        .merge(reports::router(state.clone()))
        .merge(search::router(state.clone()))
        // A review hangs off the deal it is about, not the thread the negotiation happened in.
        .merge(reviews::router(state.clone()))
        .nest("/auth", auth::router(state.clone()))
        // Reviews read as a fact about a profile, so they join the `/users` nest rather than a
        // second top-level route axum would have to resolve.
        .nest(
            "/users",
            users::router(state.clone())
                .merge(endorsements::router(state.clone()))
                .merge(reviews::user_router(state.clone())),
        )
        .nest("/posts", posts::router(state.clone()));

    r = r.route(
        "/link-preview",
        axum::routing::get(link_preview::link_preview),
    );

    if state.config.discovery.directory_enabled {
        r = r.merge(directory::router(state));
    }

    r
}
