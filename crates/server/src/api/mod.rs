mod admin;
mod reports;
// M1/M2: `pub(crate)` on these three only so `crate::tests::market` can unit-test their
// validators directly. Nothing outside the crate can reach them, and the routers are still
// mounted here.
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

// `alliances` is gone: no `mod` declaration, no route, and no file on disk. `GET /api/alliances`
// therefore 404s, which is the intended shape.

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
        // M1.2/M1.3: the public taxonomy and its admin editor. Mounted flat rather than nested
        // because the two halves live under different path prefixes (`/categories` and
        // `/admin/categories`) and behind different guards.
        .merge(categories::router(state.clone()))
        .merge(reports::router(state.clone()))
        .merge(search::router(state.clone()))
        // M3.1: writing a review hangs off the deal it is about, so it is mounted at
        // `/matches/{id}/reviews` rather than under `/conversations` — the thread is where the
        // negotiation happened, the match is what was completed.
        .merge(reviews::router(state.clone()))
        .nest("/auth", auth::router(state.clone()))
        // M3.3: reading somebody's reviews is a fact about that profile, so it joins the `/users`
        // nest beside endorsements instead of being a second top-level `/users` route — which
        // axum would have to resolve against this very nest.
        .nest(
            "/users",
            users::router(state.clone())
                .merge(endorsements::router(state.clone()))
                .merge(reviews::user_router(state.clone())),
        )
        // A3.1: posts are a flat, server-wide collection now — no tenant segment in the path.
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
